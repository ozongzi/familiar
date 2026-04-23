//! Conversation compaction — per-message summaries with raw-first loading.
//!
//! # DB schema
//!
//! `messages.summary_text`   — non-NULL on a "summary anchor" message. The
//!                             text summarises the path from the root of
//!                             the conversation up to and including this
//!                             message. Any branch whose ancestor chain
//!                             includes this message can reuse the summary.
//! `messages.summary_tokens` — estimated token count of `summary_text`,
//!                             used to decide if the summary + tail fits
//!                             when raw history doesn't.
//! `messages.context_tokens` — provider-reported input-context size on the
//!                             latest assistant message, used only to decide
//!                             whether to trigger compaction.
//!
//! # Loading policy
//!
//! [`load_for_generation`] is **raw-first**. It returns the unaltered active
//! path unless the total tokens exceed `model.compact_trigger_tokens`; only
//! then does it substitute the tail with the nearest ancestor summary.
//! Consequence: editing a message earlier than the current tip produces a
//! shorter branch that stays in raw form, without losing fidelity to the
//! summary.
//!
//! # Compaction policy
//!
//! [`maybe_compact`] **never deletes** raw messages. On trigger it finds the
//! most recent ancestor anchor (if any), summarises `prev_summary + new
//! messages since anchor`, and writes the resulting summary onto the new
//! boundary message's `summary_text`. The old anchor remains valid for any
//! branch that still traverses it.

use agentix::{LlmEvent, Message, Request, UserContent};
use futures::StreamExt;
use sqlx::PgPool;
use tracing::{info, warn};
use uuid::Uuid;

use crate::config::ModelConfig;
use crate::db::{Db, MessageRow};
use crate::worker::WorkerContext;

// ── Token thresholds ──────────────────────────────────────────────────────────

/// Max tokens the compaction model may emit for the summary. Model-independent
/// — 8K is plenty for a 9-section structured summary regardless of provider.
const COMPACT_MAX_OUTPUT_TOKENS: u32 = 8_000;

// Per-model thresholds live on `ModelConfig`:
//   - compact_trigger_tokens: context_tokens at which a compact fires AND the
//                             budget below which raw history is preferred
//   - compact_tail_tokens:    recent tail kept raw after compact

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
///
/// Raw-first: if the full active path fits under `model.compact_trigger_tokens`,
/// return it as-is. Otherwise walk the active path from the tip backward,
/// find the nearest ancestor that carries a stored summary, and splice it
/// in as a synthetic opening user message followed by every message after
/// the anchor. Falls back to raw (trusting the worker's token-budget
/// truncation) if no usable summary exists on the active path.
pub async fn load_for_generation(
    db: &Db,
    model: &ModelConfig,
    conversation_id: Uuid,
    user_id: Uuid,
) -> anyhow::Result<Vec<Message>> {
    let (rows, messages) = db
        .restore_after_rows(conversation_id, user_id, None)
        .await?;
    debug_assert_eq!(rows.len(), messages.len());

    let budget = model.compact_trigger_tokens;
    let raw_total: i64 = messages.iter().map(|m| m.estimate_tokens() as i64).sum();

    if raw_total <= budget {
        return Ok(messages);
    }

    // Walk from the tip back to the root; use the first anchor whose
    // (summary + strictly-after-tail) fits the budget.
    for i in (0..rows.len()).rev() {
        let Some(summary_text) = rows[i].summary_text.as_ref() else {
            continue;
        };
        let summary_tokens = rows[i]
            .summary_tokens
            .map(i64::from)
            .unwrap_or_else(|| summary_to_message(summary_text).estimate_tokens() as i64);
        let tail = &messages[i + 1..];
        let tail_tokens: i64 = tail.iter().map(|m| m.estimate_tokens() as i64).sum();
        if summary_tokens + tail_tokens <= budget {
            let mut out = Vec::with_capacity(tail.len() + 1);
            out.push(summary_to_message(summary_text));
            out.extend_from_slice(tail);
            return Ok(out);
        }
    }

    // No summary small enough; hand back raw and let the worker truncate.
    Ok(messages)
}

// ── Public API: compaction ────────────────────────────────────────────────────

/// Check if the conversation exceeds the compact trigger and, if so, run
/// incremental summarisation + write a new anchor on the boundary message.
/// Raw messages are never deleted: any existing anchor stays valid for
/// branches that still traverse it.
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

    // Full active path + the latest anchor on that path (if any).
    let (rows, messages) = match ctx
        .db
        .restore_after_rows(ctx.conversation_id, ctx.user_id, None)
        .await
    {
        Ok(v) => v,
        Err(e) => {
            warn!(conversation = %ctx.conversation_id, "restore failed: {e}");
            return false;
        }
    };

    let prev_anchor_idx = rows.iter().rposition(|r| r.summary_text.is_some());
    let prev_summary = prev_anchor_idx.and_then(|i| rows[i].summary_text.clone());
    let delta_start = prev_anchor_idx.map(|i| i + 1).unwrap_or(0);
    let delta_rows: &[MessageRow] = &rows[delta_start..];
    let delta_msgs: &[Message] = &messages[delta_start..];

    let split_idx = match compute_new_boundary(delta_msgs, model.compact_tail_tokens) {
        Some(v) => v,
        None => {
            info!(
                conversation = %ctx.conversation_id,
                delta_len = delta_msgs.len(),
                "nothing to summarise after boundary search"
            );
            return false;
        }
    };

    let new_until_msg_id = delta_rows[split_idx - 1].id;

    // Build summariser input: [prev_summary?, delta[..split_idx]].
    let mut input: Vec<Message> = Vec::new();
    if let Some(ref s) = prev_summary {
        input.push(summary_to_message(s));
    }
    input.extend_from_slice(&delta_msgs[..split_idx]);

    let Some(new_summary) = run_summariser(ctx, model, http, &input).await else {
        return false;
    };

    let summary_tokens = summary_to_message(&new_summary).estimate_tokens() as i32;

    if let Err(e) = ctx
        .db
        .set_summary(new_until_msg_id, &new_summary, summary_tokens)
        .await
    {
        warn!(conversation = %ctx.conversation_id, "set_summary failed: {e}");
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
        anchor_msg_id = new_until_msg_id,
        summary_chars = new_summary.len(),
        summary_tokens,
        " compaction done"
    );

    true
}

// ── Internal: boundary search ─────────────────────────────────────────────────

/// Latest provider-reported input context size on an assistant message.
async fn latest_context_tokens(pool: &PgPool, conv_id: Uuid) -> i64 {
    sqlx::query_scalar::<_, Option<i64>>(
        "SELECT context_tokens
         FROM messages
         WHERE conversation_id = $1
           AND role = 'assistant'
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

/// Walk `delta` from the back, accumulating per-message tokens, until
/// `tail_budget_tokens` is filled. Returns the split index: `delta[..split]`
/// gets summarised, `delta[split..]` stays as recent tail.
///
/// Boundary is snapped forward to a User-message boundary so the tail never
/// begins mid-turn (straddling a tool_use/tool_result pair). Returns None if
/// the whole delta fits within the tail budget (nothing to summarise) or if
/// no clean snap point exists.
fn compute_new_boundary(delta: &[Message], tail_budget_tokens: i64) -> Option<usize> {
    if delta.is_empty() {
        return None;
    }

    let mut acc: i64 = 0;
    let mut split_idx = delta.len();
    for (i, msg) in delta.iter().enumerate().rev() {
        acc += msg.estimate_tokens() as i64;
        if acc >= tail_budget_tokens {
            split_idx = i;
            break;
        }
    }

    if split_idx == 0 {
        return None;
    }

    let snapped = snap_boundary(delta, split_idx);
    if snapped >= delta.len() {
        return None;
    }
    Some(snapped)
}

/// Find a safe cut point forward of `start`. Prefers a User message; falls
/// back to the first non-ToolResult if no User is reachable. Returns
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

async fn run_summariser_http(
    ctx: &WorkerContext,
    model: &ModelConfig,
    http: &reqwest::Client,
    messages: Vec<Message>,
) -> Option<String> {
    let request = model
        .to_request()
        .system_prompt(build_compact_system_prompt())
        .messages(messages)
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
    Some(raw)
}

/// Drive `claude -p` (subprocess) with no tools and collect its text output.
/// Used when the frontier model itself is `kind=claude-code` and has no
/// HTTP credentials of its own.
async fn run_summariser_claude_code(
    ctx: &WorkerContext,
    model: &ModelConfig,
    http: &reqwest::Client,
    mut messages: Vec<Message>,
) -> Option<String> {
    // Claude Code consumes the final user turn from stdin; append an explicit
    // summary trigger so the last message is always a user turn.
    messages.push(Message::User(vec![UserContent::Text {
        text: "Now produce the summary as instructed.".to_string(),
    }]));

    let request = Request::claude_code()
        .model(&model.name)
        .system_prompt(build_compact_system_prompt())
        .messages(messages)
        .max_tokens(COMPACT_MAX_OUTPUT_TOKENS);
    let mut stream = match request.stream(http).await {
        Ok(s) => s,
        Err(e) => {
            warn!(conversation = %ctx.conversation_id, "compact (claude-code) stream failed: {e}");
            return None;
        }
    };

    let mut raw = String::new();
    while let Some(event) = stream.next().await {
        match event {
            LlmEvent::Token(t) => raw.push_str(&t),
            LlmEvent::Error(e) => {
                warn!(conversation = %ctx.conversation_id, "compact (claude-code) error: {e}");
                return None;
            }
            _ => {}
        }
    }
    Some(raw)
}

async fn run_summariser(
    ctx: &WorkerContext,
    model: &ModelConfig,
    http: &reqwest::Client,
    input: &[Message],
) -> Option<String> {
    let compact_messages = strip_images(input);

    let raw = if model.kind == "claude-code" {
        run_summariser_claude_code(ctx, model, http, compact_messages).await?
    } else {
        run_summariser_http(ctx, model, http, compact_messages).await?
    };

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
