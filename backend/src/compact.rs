//! Conversation compaction — natural-turn summarisation via a visible
//! system checkpoint inject.
//!
//! # Flow
//!
//! 1. At the end of an LLM turn in `worker::generation_loop`, after the
//!    model emits its final reply (no more tool calls), the worker checks
//!    `latest_context_tokens`. If it is `>= compact_trigger_tokens`, the
//!    worker calls [`inject_checkpoint`] which appends a real user-role
//!    message — visible in the UI, persisted in the DB — asking the model
//!    to stop and summarise.
//!
//! 2. The generation loop re-enters with this inject as the new tip. The
//!    model is given full liberty: it may still call tools to wrap up
//!    in-flight work (update TODOs, save state, …), but the prompt tells
//!    it to produce a self-contained summary as its final reply.
//!
//! 3. When the loop exits naturally (model gives final text with no tool
//!    calls), the worker calls [`finalize_anchor`] on the last assistant
//!    message. We walk backward from that message accumulating
//!    `compact_tail_tokens` worth of recent tokens, snap to a user-message
//!    boundary, and write the resulting start-of-tail id into the anchor's
//!    `summary_start_id`.
//!
//! # Loading
//!
//! [`load_for_generation`] walks the active branch from tip backward to
//! find the first message with non-NULL `summary_start_id`. That pointer
//! is the cutoff: only messages from `summary_start_id .. tip` are fed to
//! the model. Older messages stay in the DB and remain visible in the UI.
//!
//! There are no synthetic wrappers, no separate summariser model, no
//! "summary anchor" boolean column. The anchor's own `content` (whatever
//! the model wrote as its final reply) is the summary, and the pointer
//! tells the loader where to start the recent tail.

use agentix::Message;
use sqlx::PgPool;
use tracing::info;
use uuid::Uuid;

use crate::config::ModelConfig;
use crate::db::Db;
use crate::worker::WorkerContext;

/// Text appended to the active branch as a `user` message when context
/// crosses the trigger. Visible to the end user; rendered with a distinct
/// "system checkpoint" style by the frontend.
pub const CHECKPOINT_INJECT_TEXT: &str =
    "[系统检查点] 上下文接近上限。请停下手头工作，先对前文的对话进行总结，下一轮会话会丢弃一些历史信息。";

// ── Public API: trigger check + inject ────────────────────────────────────────

/// If the latest assistant turn pushed the active-branch context past
/// `compact_trigger_tokens`, append a system checkpoint as a `user`
/// message and return its id. Returns `Ok(None)` when no inject is needed.
pub async fn inject_checkpoint_if_needed(
    ctx: &WorkerContext,
    model: &ModelConfig,
) -> anyhow::Result<Option<i64>> {
    let ctx_tokens = latest_context_tokens(&ctx.pool, ctx.conversation_id).await;
    if ctx_tokens < model.compact_trigger_tokens {
        return Ok(None);
    }

    let inject_id = ctx
        .db
        .append_checkpoint_inject(ctx.conversation_id, ctx.job_id, CHECKPOINT_INJECT_TEXT)
        .await?;

    info!(
        conversation = %ctx.conversation_id,
        ctx_tokens,
        trigger = model.compact_trigger_tokens,
        inject_id,
        "⚡ compaction checkpoint injected"
    );

    Ok(Some(inject_id))
}

/// Called after a successful turn that follows a checkpoint inject. Walks
/// back from `anchor_message_id` on the active branch, accumulating raw
/// message tokens until `compact_tail_tokens` is hit; snaps the cut point
/// forward to a user-message boundary; then stamps the anchor's pointer.
pub async fn finalize_anchor(
    ctx: &WorkerContext,
    model: &ModelConfig,
    anchor_message_id: i64,
) -> anyhow::Result<()> {
    let (rows, messages) = ctx
        .db
        .restore_after_rows(ctx.conversation_id, ctx.user_id, None)
        .await?;
    debug_assert_eq!(rows.len(), messages.len());

    let anchor_idx = rows
        .iter()
        .position(|r| r.id == anchor_message_id)
        .ok_or_else(|| {
            anyhow::anyhow!("anchor {anchor_message_id} not on active branch — cannot finalize")
        })?;

    let start_idx = compute_tail_start_idx(&messages, anchor_idx, model.compact_tail_tokens);
    let snapped = snap_to_user_boundary(&messages, start_idx, anchor_idx);
    let start_id = rows[snapped].id;

    ctx.db.set_summary_start(anchor_message_id, start_id).await?;

    // Same hook as the old maybe_compact path — promote per-conversation
    // preferences / facts / procedures to user-scope when a checkpoint
    // happens, since that's a natural "the work so far is committed"
    // boundary.
    crate::spells::consolidate_conversation_memories(&ctx.pool, ctx.user_id, ctx.conversation_id)
        .await;

    info!(
        conversation = %ctx.conversation_id,
        anchor_message_id,
        start_id,
        kept_messages = anchor_idx - snapped + 1,
        "✅ compaction anchor finalized"
    );

    Ok(())
}

// ── Public API: pending-compact detection ─────────────────────────────────────

/// Returns `Some(inject_id)` when the active branch contains a
/// `[系统检查点]` user message that has no descendant anchor — i.e. a
/// compact attempt that was started but never finalized. Used by the
/// send-message guard (block new user input until retry succeeds) and by
/// the worker (resume the compact directly instead of injecting a second
/// checkpoint).
pub async fn pending_compact_inject_id(
    db: &Db,
    conversation_id: Uuid,
    user_id: Uuid,
) -> anyhow::Result<Option<i64>> {
    let (rows, _msgs) = db.restore_after_rows(conversation_id, user_id, None).await?;

    // Walk from tip back to find the most recent inject.
    let inject_idx = (0..rows.len()).rev().find(|&i| {
        rows[i].role == "user"
            && rows[i]
                .content
                .as_deref()
                .is_some_and(|c| c.starts_with(CHECKPOINT_INJECT_TEXT_PREFIX))
    });

    let Some(idx) = inject_idx else {
        return Ok(None);
    };

    // If any message at-or-after the inject is itself an anchor, the
    // compact finalised — nothing pending. Otherwise it's pending.
    let finalised = rows[idx..].iter().any(|r| r.summary_start_id.is_some());
    if finalised {
        Ok(None)
    } else {
        Ok(Some(rows[idx].id))
    }
}

/// Stable prefix of `CHECKPOINT_INJECT_TEXT` used to detect inject messages
/// regardless of any trailing prompt tuning. Kept narrow so a normal user
/// message that happens to start with `[系统检查点]` would still match —
/// which we accept as a deliberate signal.
const CHECKPOINT_INJECT_TEXT_PREFIX: &str = "[系统检查点]";

// ── Public API: load history for the LLM ──────────────────────────────────────
/// Load the message history the worker should feed to the LLM. Walks the
/// active branch from tip back; the first message with a non-NULL
/// `summary_start_id` defines the cutoff. Returns the chain
/// `[summary_start, ..., tip]` verbatim.
pub async fn load_for_generation(
    db: &Db,
    _model: &ModelConfig,
    conversation_id: Uuid,
    user_id: Uuid,
) -> anyhow::Result<Vec<Message>> {
    let (rows, messages) = db
        .restore_after_rows(conversation_id, user_id, None)
        .await?;
    debug_assert_eq!(rows.len(), messages.len());

    // First anchor walking from tip backwards = the most recent one.
    let anchor_idx = (0..rows.len()).rev().find(|&i| rows[i].summary_start_id.is_some());

    if let Some(idx) = anchor_idx {
        let cutoff_id = rows[idx].summary_start_id.unwrap();
        if let Some(cutoff_idx) = rows.iter().position(|r| r.id == cutoff_id) {
            return Ok(messages[cutoff_idx..].to_vec());
        }
        // Cutoff message missing from active branch (shouldn't happen with
        // ON DELETE SET NULL, but be defensive): fall through to raw.
    }

    Ok(messages)
}

// ── Internals ─────────────────────────────────────────────────────────────────

/// Provider-reported input-context size on the latest assistant message
/// of the active branch. Used to decide if compaction should fire.
async fn latest_context_tokens(pool: &PgPool, conv_id: Uuid) -> i64 {
    sqlx::query_scalar::<_, Option<i64>>(
        "WITH RECURSIVE branch AS (
             SELECT m.id, m.parent_id, m.role, m.context_tokens
             FROM messages m
             JOIN conversations c ON c.id = $1
             WHERE m.id = c.active_message_id
             UNION ALL
             SELECT m.id, m.parent_id, m.role, m.context_tokens
             FROM messages m
             JOIN branch b ON m.id = b.parent_id
         )
         SELECT context_tokens
         FROM branch
         WHERE role = 'assistant'
           AND context_tokens IS NOT NULL
         ORDER BY id DESC LIMIT 1",
    )
    .bind(conv_id)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
    .flatten()
    .unwrap_or(0)
}

/// Walk back from `anchor_idx` (inclusive), accumulating tokens until the
/// next message would push the total over `tail_budget`. Returns the
/// earliest index that still fits — i.e. the candidate start of the tail.
fn compute_tail_start_idx(
    messages: &[Message],
    anchor_idx: usize,
    tail_budget: i64,
) -> usize {
    let mut acc: i64 = 0;
    let mut start = anchor_idx;
    for i in (0..=anchor_idx).rev() {
        let toks = messages[i].estimate_tokens() as i64;
        if acc + toks > tail_budget && i < anchor_idx {
            // Adding this one would overflow. Stop, keep `start` at i+1.
            break;
        }
        acc += toks;
        start = i;
    }
    start
}

/// Move `start_idx` forward to the nearest User-role message, but never
/// past `anchor_idx`. This keeps the cutoff from landing in the middle of
/// a tool_use/tool_result pair, which providers reject.
fn snap_to_user_boundary(messages: &[Message], start_idx: usize, anchor_idx: usize) -> usize {
    let slice = &messages[start_idx..=anchor_idx];
    if let Some(off) = slice.iter().position(|m| matches!(m, Message::User(_))) {
        return start_idx + off;
    }
    // No user message between start and anchor — fall back to the first
    // non-ToolResult, which is always a safe boundary.
    if let Some(off) = slice
        .iter()
        .position(|m| !matches!(m, Message::ToolResult { .. }))
    {
        return start_idx + off;
    }
    // Pathological case (all tool results): point at the anchor itself.
    anchor_idx
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentix::{Message, UserContent};

    fn user_msg(text: impl Into<String>) -> Message {
        Message::User(vec![UserContent::Text { text: text.into() }])
    }

    fn assistant_msg(text: impl Into<String>) -> Message {
        Message::Assistant {
            content: Some(text.into()),
            reasoning: None,
            tool_calls: vec![],
            provider_data: None,
        }
    }

    fn tool_result(text: impl Into<String>) -> Message {
        Message::ToolResult {
            call_id: "x".to_string(),
            content: vec![agentix::Content::text(text.into())],
        }
    }

    #[test]
    fn tail_start_respects_budget_and_includes_anchor() {
        // Each message is short; budget large enough for all of them.
        let msgs = vec![
            user_msg("a"),
            assistant_msg("b"),
            user_msg("c"),
            assistant_msg("d"),
        ];
        let start = compute_tail_start_idx(&msgs, 3, 10_000);
        assert_eq!(start, 0, "huge budget keeps everything");
    }

    #[test]
    fn tail_start_trims_when_budget_exceeded() {
        // Big messages, small budget — only the anchor fits.
        let big = "x".repeat(10_000);
        let msgs = vec![
            user_msg(big.clone()),
            assistant_msg(big.clone()),
            user_msg(big.clone()),
            assistant_msg(big.clone()),
        ];
        let start = compute_tail_start_idx(&msgs, 3, 100);
        assert_eq!(start, 3, "tiny budget keeps only the anchor itself");
    }

    #[test]
    fn snap_skips_into_user_boundary() {
        let msgs = vec![
            assistant_msg("a"),
            tool_result("b"),
            user_msg("c"),
            assistant_msg("d"),
        ];
        // start=0 (assistant) → snap forward to user at idx 2.
        assert_eq!(snap_to_user_boundary(&msgs, 0, 3), 2);
    }

    #[test]
    fn snap_keeps_user_if_already_at_user() {
        let msgs = vec![user_msg("a"), assistant_msg("b")];
        assert_eq!(snap_to_user_boundary(&msgs, 0, 1), 0);
    }

    #[test]
    fn snap_fallback_when_no_user_exists() {
        // anchor=2 (assistant). Between idx 1 (tool result) and idx 2 there
        // is no user, but snap should fall back to the assistant.
        let msgs = vec![tool_result("a"), tool_result("b"), assistant_msg("c")];
        assert_eq!(snap_to_user_boundary(&msgs, 0, 2), 2);
    }
}
