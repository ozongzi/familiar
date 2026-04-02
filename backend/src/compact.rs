//! Conversation compaction — summary generation.
//!
//! When conversation history approaches the token budget, `try_compact` is
//! called.  It sends the full history to the cheap model with a plain-markdown
//! summary prompt (no XML, no JSON — avoids DeepSeek function-call hallucination),
//! stores the formatted summary in `conversations.compact_summary`, and returns
//! the text so the caller can inject it as the first user message.
//! The original message history is NOT truncated here — that happens in
//! `generation_loop` via `truncate_to_token_budget` as before.

use agentix::{LlmEvent, Message, UserContent};
use futures::StreamExt;
use tracing::{info, warn};
use uuid::Uuid;

use crate::config::ModelConfig;
use crate::worker::WorkerContext;

// ── Token thresholds ──────────────────────────────────────────────────────────

/// Fraction of HISTORY_TOKEN_BUDGET at which we trigger compaction.
const COMPACT_TRIGGER_FRACTION: f64 = 0.80;
pub const HISTORY_TOKEN_BUDGET: usize = 25_000;
const COMPACT_TRIGGER_TOKENS: usize =
    (HISTORY_TOKEN_BUDGET as f64 * COMPACT_TRIGGER_FRACTION) as usize;

/// Max tokens the compaction model may emit for the summary.
const COMPACT_MAX_OUTPUT_TOKENS: u32 = 8_000;

// ── Compact prompt ────────────────────────────────────────────────────────────

/// Preamble placed FIRST to suppress tool calls on DeepSeek and other models
/// that hallucinate function calls when they see long context + XML tags.
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

/// Minimal cleanup: strip any accidental tool-call leakage, collapse excess blank lines.
/// Since we now ask for pure Markdown, there's nothing to parse — output is the summary.
pub fn format_compact_summary(raw: &str) -> String {
    // Remove any function_call / tool_use blocks the model might have hallucinated
    let cleaned = strip_tool_call_blocks(raw);

    // Collapse 3+ consecutive newlines to 2
    let collapsed = collapse_blank_lines(&cleaned);

    // Ensure it starts with "Summary:\n" for consistency with injection wrapper
    let trimmed = collapsed.trim();
    if trimmed.starts_with("## 1.") || trimmed.starts_with("# Summary") {
        format!("Summary:\n{trimmed}")
    } else if trimmed.starts_with("Summary:") {
        trimmed.to_string()
    } else {
        format!("Summary:\n{trimmed}")
    }
}

/// Strip `<function_calls>…</function_calls>` and DeepSeek DSML blocks if the
/// model hallucinated them despite instructions.
fn strip_tool_call_blocks(s: &str) -> String {
    let mut result = s.to_string();
    for tag in &["function_calls", "tool_use", "tool_call"] {
        let open = format!("<{tag}");
        let close = format!("</{tag}>");
        loop {
            if let Some(start) = result.find(&open) {
                if let Some(rel_end) = result[start..].find(&close) {
                    let end = start + rel_end + close.len();
                    result.replace_range(start..end, "");
                } else {
                    // Unclosed tag — strip from open to end
                    result.truncate(start);
                    break;
                }
            } else {
                break;
            }
        }
    }
    // Strip DeepSeek DSML marker and anything after it
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

/// Wrap a stored summary for injection as the opening user message of a new
/// context window.
pub fn compact_summary_to_user_message(summary: &str) -> String {
    format!(
        "## 对话摘要（早期上下文已压缩）\n\
本对话从一个已达到上下文上限的先前会话延续。以下摘要涵盖了早期对话部分。\n\
\n\
{summary}\n\
\n\
继续从中断处继续工作，无需向用户询问任何问题。直接恢复 — 不要确认摘要，不要重述发生了什么。"
    )
}

// ── Rough token estimation ────────────────────────────────────────────────────

fn rough_token_count(messages: &[Message]) -> usize {
    messages.iter().fold(0usize, |acc, m| {
        let text = match m {
            Message::User(parts) => parts.iter().fold(String::new(), |mut s, p| {
                if let UserContent::Text { text } = p {
                    s.push_str(text);
                }
                s
            }),
            Message::Assistant {
                content,
                tool_calls,
                ..
            } => {
                let mut s = content.clone().unwrap_or_default();
                for tc in tool_calls {
                    s.push_str(&tc.arguments);
                }
                s
            }
            Message::ToolResult { content, .. } => content.clone(),
        };
        acc + text.len() / 4
    })
}

// ── Public API ────────────────────────────────────────────────────────────────

pub fn should_compact(messages: &[Message]) -> bool {
    rough_token_count(messages) >= COMPACT_TRIGGER_TOKENS
}

/// Strip images from messages before sending for compaction.
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

/// Run compaction:
/// 1. Sends full history to cheap model with compact system prompt (plain Markdown,
///    no XML/JSON to avoid DeepSeek function-call hallucination).
/// 2. Strips any accidental tool-call leakage from the output.
/// 3. Persists the summary in `conversations.compact_summary`.
/// 4. Emits a `{"type":"compact","summary":"..."}` SSE event.
/// 5. Returns the formatted summary so the caller can inject it as the first
///    user message for the next generation turn.
///
/// On any failure logs a warning and returns `None` — caller falls back to truncation.
pub async fn try_compact(
    ctx: &WorkerContext,
    messages: &[Message],
    cheap_model: &ModelConfig,
    http: &reqwest::Client,
) -> Option<String> {
    info!(
        conversation = %ctx.conversation_id,
        msgs = messages.len(),
        "⚡ triggering compaction"
    );

    let compact_messages = strip_images(messages);

    let request = cheap_model
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

    // If despite instructions the model output only tool calls, discard
    let looks_like_only_tool_calls = {
        let stripped = strip_tool_call_blocks(&raw);
        stripped.trim().is_empty()
    };
    if looks_like_only_tool_calls {
        warn!(conversation = %ctx.conversation_id, "compact output was entirely tool calls, discarding");
        return None;
    }

    let formatted = format_compact_summary(&raw);

    if formatted.trim().is_empty() || formatted == "Summary:" {
        warn!(conversation = %ctx.conversation_id, "compact produced empty summary after formatting");
        return None;
    }

    // Persist to DB
    let _ = sqlx::query(
        "UPDATE conversations SET compact_summary = $1, compact_at = NOW() WHERE id = $2",
    )
    .bind(&formatted)
    .bind(ctx.conversation_id)
    .execute(&ctx.pool)
    .await;

    // Promote high-value conversation memories to user scope
    crate::spells::consolidate_conversation_memories(&ctx.pool, ctx.user_id, ctx.conversation_id)
        .await;

    // Emit SSE event so the frontend knows compaction happened
    crate::worker::emit(
        ctx,
        serde_json::json!({"type": "compact", "summary": &formatted}),
    )
    .await;

    info!(
        conversation = %ctx.conversation_id,
        chars = formatted.len(),
        "✅ compaction done"
    );

    Some(formatted)
}

/// Load any existing compact summary for this conversation from DB.
pub async fn load_compact_summary(pool: &sqlx::PgPool, conversation_id: Uuid) -> Option<String> {
    sqlx::query_scalar::<_, String>(
        "SELECT compact_summary FROM conversations WHERE id = $1 AND compact_summary IS NOT NULL",
    )
    .bind(conversation_id)
    .fetch_optional(pool)
    .await
    .unwrap_or(None)
}
