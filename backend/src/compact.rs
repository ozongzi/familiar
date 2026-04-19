//! Conversation compaction — self-contained summary generation + unified load.
//!
//! # DB schema
//!
//! `conversations.compact_summary`    — the summary text (covers all messages
//!                                       up to compact_until_msg_id)
//! `conversations.compact_until_msg_id` — boundary: messages with id <= this
//!                                       are summarised; id > this is the
//!                                       recent tail kept raw
//! `messages.prompt_tokens` + friends — per-assistant-message provider tokens,
//!                                       used to locate the boundary precisely
//!
//! # Entry points
//!
//! - [`load_for_generation`] returns the message history the worker should
//!   feed to the LLM: `[summary_msg?] + recent_tail`.  No other code needs to
//!   know the summary exists.
//! - [`maybe_compact`] runs compaction if the current context exceeds the
//!   trigger threshold, updates the DB, and emits an SSE event.  Incremental:
//!   each run summarises `prev_summary + messages_since_last_compact`.
//!
//! All constants (trigger, budgets, max output) live here; the worker doesn't
//! know or care.

use agentix::{LlmEvent, Message, UserContent};
use futures::StreamExt;
use sqlx::PgPool;
use tracing::{info, warn};
use uuid::Uuid;

use crate::config::ModelConfig;
use crate::db::Db;
use crate::worker::WorkerContext;

// ── Token thresholds ──────────────────────────────────────────────────────────

/// Max tokens the compaction model may emit for the summary. Model-independent
/// — 8K is plenty for a 9-section structured summary regardless of provider.
const COMPACT_MAX_OUTPUT_TOKENS: u32 = 8_000;

// Per-model thresholds live on `ModelConfig`:
//   - compact_trigger_tokens: prompt_tokens at which a compact fires
//   - compact_tail_tokens:    recent tail kept raw after compact
// The worker derives its truncate safety ceiling from the trigger.

// ── Compact prompt ────────────────────────────────────────────────────────────

const NO_TOOLS_PREAMBLE: &str = "CRITICAL: Respond with TEXT ONLY. Do NOT call any tools or functions.\n\
Do NOT use <function_calls>, <tool_use>, or any similar syntax.\n\
Do NOT output JSON or XML. Output plain Markdown only.\n\
Tool calls will be ignored and you will fail the task.\n\n";

const COMPACT_PROMPT: &str = "Your task is to create a detailed summary of the conversation so far, \
paying close attention to the user's explicit requests and your previous actions.\n\
This summary should be thorough in capturing technical details, code patterns, and architectural \
decisions that would be essential for continuing development work without losing context.\n\
\n\
Your summary MUST follow this exact Markdown structure:\n\
\n\
## 1. Primary Request and Intent\n\
[Capture all of the user's explicit requests and intents in detail]\n\
\n\
## 2. Key Technical Concepts\n\
[List all important technical concepts, technologies, and frameworks discussed]\n\
\n\
## 3. Files and Code Sections\n\
[Enumerate specific files and code sections examined, modified, or created.\n\
Include full code snippets where applicable and explain why each is important.]\n\
\n\
## 4. Errors and Fixes\n\
[List all errors encountered and how they were fixed.\n\
Pay special attention to user feedback, especially if the user told you to do something differently.]\n\
\n\
## 5. Problem Solving\n\
[Document problems solved and any ongoing troubleshooting efforts]\n\
\n\
## 6. All User Messages\n\
[List ALL user messages that are not tool results. These are critical for understanding\n\
the user's feedback and changing intent.]\n\
\n\
## 7. Pending Tasks\n\
[Outline any pending tasks you have explicitly been asked to work on]\n\
\n\
## 8. Current Work\n\
[Describe in detail precisely what was being worked on immediately before this summary request,\n\
paying special attention to the most recent messages. Include file names and code snippets.]\n\
\n\
## 9. Optional Next Step\n\
[The next step directly in line with the user's most recent explicit request.\n\
Include direct quotes from the most recent conversation showing exactly what task you were\n\
working on and where you left off. If the last task was concluded, only list next steps\n\
explicitly requested by the user.]\n\
\n\
Please provide your summary based on the conversation so far, following this structure exactly.\n\
Output only the Markdown summary — no preamble, no commentary, no tool calls.\n";

const NO_TOOLS_TRAILER: &str = "\n\nREMINDER: Output ONLY the Markdown summary with the 9 sections above. \
No tool calls. No JSON. No XML. No text before section 1.";

fn build_compact_system_prompt() -> String {
    format!("{NO_TOOLS_PREAMBLE}{COMPACT_PROMPT}{NO_TOOLS_TRAILER}")
}

// ── Summary formatting ────────────────────────────────────────────────────────

fn format_compact_summary(raw: &str) -> String {
    let cleaned = strip_tool_call_blocks(raw);
    let collapsed = collapse_blank_lines(&cleaned);
    let trimmed = collapsed.trim();
    if trimmed.starts_with("## 1.") || trimmed.starts_with("# Summary") {
        format!("Summary:\n{trimmed}")
    } else if trimmed.starts_with("Summary:") {
        trimmed.to_string()
    } else {
        format!("Summary:\n{trimmed}")
    }
}

fn strip_tool_call_blocks(s: &str) -> String {
    let mut result = s.to_string();
    for tag in &["function_calls", "tool_use", "tool_call"] {
        let open = format!("<{tag}");
        let close = format!("</{tag}>");
        while let Some(start) = result.find(&open) {
            if let Some(rel_end) = result[start..].find(&close) {
                let end = start + rel_end + close.len();
                result.replace_range(start..end, "");
            } else {
                result.truncate(start);
                break;
            }
        }
    }
    if let Some(pos) = result.find("<｜DSML｜") {
        result.truncate(pos);
    }
    result
}

fn collapse_blank_lines(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut blank_count = 0usize;
    for line in s.split('\n') {
        if line.trim().is_empty() {
            blank_count += 1;
            if blank_count <= 2 {
                out.push('\n');
            }
        } else {
            blank_count = 0;
            out.push_str(line);
            out.push('\n');
        }
    }
    out.trim().to_string()
}

/// Wrap a stored summary for injection as the opening user message.
fn summary_to_user_message_text(summary: &str) -> String {
    format!(
        "## 对话摘要（早期上下文已压缩）\n\
本对话从一个已达到上下文上限的先前会话延续。以下摘要涵盖了早期对话部分。\n\
\n\
{summary}\n\
\n\
继续从中断处继续工作，无需向用户询问任何问题。直接恢复 — 不要确认摘要，不要重述发生了什么。"
    )
}

fn summary_to_message(summary: &str) -> Message {
    Message::User(vec![UserContent::Text {
        text: summary_to_user_message_text(summary),
    }])
}

// ── Strip images before summarisation ────────────────────────────────────────

fn strip_images(messages: &[Message]) -> Vec<Message> {
    messages
        .iter()
        .map(|m| match m {
            Message::User(parts) => {
                let new_parts: Vec<UserContent> = parts
                    .iter()
                    .map(|p| match p {
                        UserContent::Image(_) => UserContent::Text {
                            text: "[image]".to_string(),
                        },
                        other => other.clone(),
                    })
                    .collect();
                Message::User(new_parts)
            }
            other => other.clone(),
        })
        .collect()
}

// ── Public API: unified loader ────────────────────────────────────────────────

/// Load the message history the worker should feed to the LLM.
/// Transparently prepends the compact summary (if any) and returns only the
/// recent tail — worker doesn't need to know compaction happened.
pub async fn load_for_generation(
    db: &Db,
    conversation_id: Uuid,
    user_id: Uuid,
) -> anyhow::Result<Vec<Message>> {
    let state = load_compact_state(&db.pool, conversation_id).await;
    match state {
        Some((summary, until_id)) => {
            let recent = db
                .restore_after(conversation_id, user_id, Some(until_id))
                .await?;
            let mut out = Vec::with_capacity(recent.len() + 1);
            out.push(summary_to_message(&summary));
            out.extend(recent);
            Ok(out)
        }
        None => db.restore(conversation_id, user_id).await,
    }
}

// ── Public API: compaction ────────────────────────────────────────────────────

/// Check if the conversation exceeds the compact trigger and, if so, run
/// incremental summarisation + update DB.  Returns `true` if a compact ran.
pub async fn maybe_compact(
    ctx: &WorkerContext,
    model: &ModelConfig,
    http: &reqwest::Client,
) -> bool {
    let ctx_tokens = latest_context_tokens(&ctx.pool, ctx.conversation_id).await;
    if ctx_tokens < model.compact_trigger_tokens {
        return false;
    }

    info!(
        conversation = %ctx.conversation_id,
        ctx_tokens,
        "⚡ compaction threshold reached"
    );

    // Load state needed for incremental summarisation.
    let prev_state = load_compact_state(&ctx.pool, ctx.conversation_id).await;
    let prev_until = prev_state.as_ref().map(|(_, id)| *id);
    let prev_summary = prev_state.as_ref().map(|(s, _)| s.clone());

    // All messages since the last compact boundary (or the whole conversation
    // if no prior compact).
    let delta = match ctx
        .db
        .restore_after(ctx.conversation_id, ctx.user_id, prev_until)
        .await
    {
        Ok(v) => v,
        Err(e) => {
            warn!(conversation = %ctx.conversation_id, "restore_after failed: {e}");
            return false;
        }
    };

    // Pick a new boundary: keep `RECENT_MESSAGES_BUDGET` tokens of delta as
    // recent tail, summarise everything before.  Returns the new boundary
    // msg_id and the slice of delta to feed into the summariser.
    let (new_until_id, to_summarise_range) =
        match compute_new_boundary(&ctx.pool, ctx.conversation_id, prev_until, &delta, model.compact_tail_tokens).await {
            Some(v) => v,
            None => {
                info!(
                    conversation = %ctx.conversation_id,
                    "nothing to summarise after boundary search"
                );
                return false;
            }
        };

    // Build input to the summariser: prev_summary (if any) + the messages up
    // to the new boundary.  prev_summary replaces the old history so this is
    // incremental.
    let mut input: Vec<Message> = Vec::new();
    if let Some(ref s) = prev_summary {
        input.push(summary_to_message(s));
    }
    input.extend_from_slice(&delta[..to_summarise_range]);

    let Some(new_summary) = run_summariser(ctx, model, http, &input).await else {
        return false;
    };

    // Persist to DB
    let res = sqlx::query(
        "UPDATE conversations
         SET compact_summary      = $1,
             compact_until_msg_id = $2,
             compact_at           = NOW()
         WHERE id = $3",
    )
    .bind(&new_summary)
    .bind(new_until_id)
    .bind(ctx.conversation_id)
    .execute(&ctx.pool)
    .await;

    if let Err(e) = res {
        warn!(conversation = %ctx.conversation_id, "compact DB update failed: {e}");
        return false;
    }

    crate::spells::consolidate_conversation_memories(&ctx.pool, ctx.user_id, ctx.conversation_id)
        .await;

    crate::worker::emit(
        ctx,
        serde_json::json!({"type": "compact", "summary": &new_summary}),
    )
    .await;

    info!(
        conversation = %ctx.conversation_id,
        new_until_msg_id = new_until_id,
        summary_chars = new_summary.len(),
        "✅ compaction done"
    );

    true
}

// ── Internal: DB queries ──────────────────────────────────────────────────────

async fn load_compact_state(pool: &PgPool, conv_id: Uuid) -> Option<(String, i64)> {
    let row: Option<(Option<String>, Option<i64>)> = sqlx::query_as(
        "SELECT compact_summary, compact_until_msg_id FROM conversations WHERE id = $1",
    )
    .bind(conv_id)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    match row {
        Some((Some(s), Some(id))) => Some((s, id)),
        _ => None,
    }
}

/// Latest provider-reported prompt_tokens on an assistant message — this is
/// the current context size the LLM sees.
async fn latest_context_tokens(pool: &PgPool, conv_id: Uuid) -> i64 {
    sqlx::query_scalar::<_, Option<i64>>(
        "SELECT prompt_tokens FROM messages
         WHERE conversation_id = $1
           AND role = 'assistant'
           AND prompt_tokens IS NOT NULL
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

/// Walk the delta from the back, accumulating per-message tokens (provider
/// counts where available, tiktoken estimate otherwise), until we fill
/// `RECENT_MESSAGES_BUDGET`.  Returns `(new_until_msg_id, split_index)` where
/// `delta[..split_index]` is what gets summarised.
///
/// The boundary is snapped so the recent tail always starts with a User
/// message (never splits a tool_use/tool_result pair or starts mid-group).
async fn compute_new_boundary(
    pool: &PgPool,
    conv_id: Uuid,
    prev_until: Option<i64>,
    delta: &[Message],
    tail_budget_tokens: i64,
) -> Option<(i64, usize)> {
    if delta.is_empty() {
        return None;
    }

    // Fetch ids + per-msg tokens for the delta in order.  We rebuild this
    // mapping rather than trusting array alignment because restore_after
    // filters out streaming / empty rows.
    let rows: Vec<(i64, String, Option<i64>, Option<i64>)> = sqlx::query_as(
        r#"
        WITH RECURSIVE branch AS (
            SELECT m.id, m.role, m.parent_id, m.prompt_tokens, m.completion_tokens,
                   m.streaming, m.content, m.spell_casts
            FROM messages m
            JOIN conversations c ON c.id = $1
            WHERE m.id = c.active_message_id AND m.id > $2
            UNION ALL
            SELECT m.id, m.role, m.parent_id, m.prompt_tokens, m.completion_tokens,
                   m.streaming, m.content, m.spell_casts
            FROM messages m
            JOIN branch b ON m.id = b.parent_id
            WHERE m.id > $2
        )
        SELECT id, role, prompt_tokens, completion_tokens
        FROM branch
        WHERE streaming = false
          AND NOT (role = 'assistant'
                   AND (content IS NULL OR content = '')
                   AND spell_casts IS NULL)
        ORDER BY id ASC
        "#,
    )
    .bind(conv_id)
    .bind(prev_until.unwrap_or(i64::MIN))
    .fetch_all(pool)
    .await
    .ok()?;

    if rows.len() != delta.len() {
        warn!(
            conv_id = %conv_id,
            rows = rows.len(),
            delta = delta.len(),
            "row/delta length mismatch; aborting compact"
        );
        return None;
    }

    // Walk from the back, estimating per-message tokens.
    // For assistant: use (next_assistant.prompt_tokens - this.prompt_tokens)
    // as the size of (this assistant's output + user/tool msgs in between).
    // Simpler: use completion_tokens + tiktoken(everything between). But
    // tiktoken is available as agentix::Message::estimate_tokens, so just use
    // that per-message. Provider counts are used for the trigger; boundary
    // accuracy doesn't need to be exact.
    let mut acc: i64 = 0;
    let mut split_idx = delta.len();
    for (i, msg) in delta.iter().enumerate().rev() {
        let est = msg.estimate_tokens() as i64;
        acc += est;
        if acc >= tail_budget_tokens {
            split_idx = i;
            break;
        }
    }

    // split_idx == 0 means even the whole delta fits within the recent budget
    // — nothing to summarise this round.
    if split_idx == 0 {
        return None;
    }

    // Snap forward to a clean boundary: prefer a User message (cleanest —
    // recent starts with a new user turn).  If no User is reachable (e.g.,
    // a long agent tool chain with no user intervention), fall back to
    // skipping past orphan ToolResults so the tool_use/tool_result pair
    // doesn't straddle the summary/recent boundary.
    let snapped = snap_boundary(delta, split_idx);
    if snapped >= delta.len() {
        // The whole tail was ToolResults; can't split cleanly this round.
        return None;
    }

    // new_until_msg_id = last message that gets summarised (id of delta[snapped - 1]).
    let new_until_id = rows[snapped - 1].0;
    Some((new_until_id, snapped))
}

/// Find a safe cut point forward of `start`.  Prefers a User message; falls
/// back to the first non-ToolResult if no User is reachable.  Returns
/// `delta.len()` if every message from `start` onwards is a ToolResult.
fn snap_boundary(delta: &[Message], start: usize) -> usize {
    for i in start..delta.len() {
        if matches!(delta[i], Message::User(_)) {
            return i;
        }
    }
    let mut idx = start;
    while idx < delta.len() {
        if !matches!(delta[idx], Message::ToolResult { .. }) {
            return idx;
        }
        idx += 1;
    }
    delta.len()
}

// ── Internal: summariser ──────────────────────────────────────────────────────

async fn run_summariser(
    ctx: &WorkerContext,
    model: &ModelConfig,
    http: &reqwest::Client,
    input: &[Message],
) -> Option<String> {
    let compact_messages = strip_images(input);

    let request = model
        .to_request()
        .system_prompt(build_compact_system_prompt())
        .messages(compact_messages)
        .max_tokens(COMPACT_MAX_OUTPUT_TOKENS);

    let mut stream = match request.stream(http).await {
        Ok(s) => s,
        Err(e) => {
            warn!(conversation = %ctx.conversation_id, "compact stream failed: {e}");
            return None;
        }
    };

    let mut raw = String::new();
    while let Some(event) = stream.next().await {
        match event {
            LlmEvent::Token(t) => raw.push_str(&t),
            LlmEvent::Error(e) => {
                warn!(conversation = %ctx.conversation_id, "compact stream error: {e}");
                return None;
            }
            _ => {}
        }
    }

    if raw.trim().is_empty() {
        warn!(conversation = %ctx.conversation_id, "compact produced empty output");
        return None;
    }

    if strip_tool_call_blocks(&raw).trim().is_empty() {
        warn!(conversation = %ctx.conversation_id, "compact output was entirely tool calls, discarding");
        return None;
    }

    let formatted = format_compact_summary(&raw);
    if formatted.trim().is_empty() || formatted == "Summary:" {
        warn!(conversation = %ctx.conversation_id, "compact produced empty summary after formatting");
        return None;
    }
    Some(formatted)
}
