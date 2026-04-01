//! Conversation compaction — CC-style summary generation.
//!
//! When conversation history approaches the token budget, `try_compact` is
//! called.  It sends the full history to a cheap model with a structured
//! summary prompt (9-section CC format), strips the `<analysis>` scratchpad,
//! stores the formatted summary in `conversations.compact_summary`, and
//! returns the formatted text so the caller can inject it into the system
//! prompt.  The original message history is NOT truncated here — that happens
//! in `generation_loop` via `truncate_to_token_budget` as before.

use agentix::{LlmEvent, Message, UserContent};
use futures::StreamExt;
use tracing::{info, warn};
use uuid::Uuid;

use crate::config::ModelConfig;
use crate::worker::WorkerContext;

// ── Token thresholds ──────────────────────────────────────────────────────────

/// Fraction of HISTORY_TOKEN_BUDGET at which we trigger compaction.
/// At 80 % of 25 k = 20 k tokens we compact.
const COMPACT_TRIGGER_FRACTION: f64 = 0.80;
pub const HISTORY_TOKEN_BUDGET: usize = 25_000;
const COMPACT_TRIGGER_TOKENS: usize =
    (HISTORY_TOKEN_BUDGET as f64 * COMPACT_TRIGGER_FRACTION) as usize;

/// Max tokens the compaction model may emit for the summary.
const COMPACT_MAX_OUTPUT_TOKENS: u32 = 8_000;

// ── Compact prompt ────────────────────────────────────────────────────────────

const NO_TOOLS_PREAMBLE: &str = "CRITICAL: Respond with a single JSON object ONLY. No markdown fences, no prose outside the JSON.\n\
\n\
- Do NOT call any tools.\n\
- Your entire response must be exactly one JSON object with two keys: \"analysis\" and \"summary\".\n\
- Example: {\"analysis\": \"...\", \"summary\": \"...\"}\n\
\n";

const COMPACT_PROMPT: &str = "Your task is to create a detailed summary of the conversation so far, \
paying close attention to the user's explicit requests and your previous actions.\n\
This summary should be thorough in capturing technical details, code patterns, and architectural \
decisions that would be essential for continuing development work without losing context.\n\
\n\
Before providing your final summary, wrap your analysis in <analysis> tags to organize your \
thoughts and ensure you've covered all necessary points. In your analysis process:\n\
\n\
1. Chronologically analyze each message and section of the conversation. For each section \
thoroughly identify:\n\
   - The user's explicit requests and intents\n\
   - Your approach to addressing the user's requests\n\
   - Key decisions, technical concepts and code patterns\n\
   - Specific details like:\n\
     - file names\n\
     - full code snippets\n\
     - function signatures\n\
     - file edits\n\
   - Errors that you ran into and how you fixed them\n\
   - Pay special attention to specific user feedback that you received, especially if the user \
told you to do something differently.\n\
2. Double-check for technical accuracy and completeness, addressing each required element \
thoroughly.\n\
\n\
Your summary should include the following sections:\n\
\n\
1. Primary Request and Intent: Capture all of the user's explicit requests and intents in detail\n\
2. Key Technical Concepts: List all important technical concepts, technologies, and frameworks \
discussed.\n\
3. Files and Code Sections: Enumerate specific files and code sections examined, modified, or \
created. Pay special attention to the most recent messages and include full code snippets where \
applicable and include a summary of why this file read or edit is important.\n\
4. Errors and fixes: List all errors that you ran into, and how you fixed them. Pay special \
attention to specific user feedback that you received, especially if the user told you to do \
something differently.\n\
5. Problem Solving: Document problems solved and any ongoing troubleshooting efforts.\n\
6. All user messages: List ALL user messages that are not tool results. These are critical for \
understanding the users' feedback and changing intent.\n\
7. Pending Tasks: Outline any pending tasks that you have explicitly been asked to work on.\n\
8. Current Work: Describe in detail precisely what was being worked on immediately before this \
summary request, paying special attention to the most recent messages from both user and \
assistant. Include file names and code snippets where applicable.\n\
9. Optional Next Step: List the next step that you will take that is related to the most recent \
work you were doing. IMPORTANT: ensure that this step is DIRECTLY in line with the user's most \
recent explicit requests, and the task you were working on immediately before this summary \
request. If your last task was concluded, then only list next steps if they are explicitly in \
line with the users request. Do not start on tangential requests or really old requests that \
were already completed without confirming with the user first.\n\
                       If there is a next step, include direct quotes from the most recent \
conversation showing exactly what task you were working on and where you left off. This should \
be verbatim to ensure there's no drift in task interpretation.\n\
\n\
Please provide your summary based on the conversation so far, following this structure and \
ensuring precision and thoroughness in your response.\n";

const NO_TOOLS_TRAILER: &str = "\n\nREMINDER: Output ONLY a JSON object {\"analysis\": \"...\", \"summary\": \"...\"}. \
No tool calls. No markdown. No text outside the JSON object.";

fn build_compact_system_prompt() -> String {
    format!("{NO_TOOLS_PREAMBLE}{COMPACT_PROMPT}{NO_TOOLS_TRAILER}")
}

// ── Summary formatting ────────────────────────────────────────────────────────

/// Parse a JSON compact response `{"analysis": "...", "summary": "..."}` and return the summary.
fn parse_compact_json(raw: &str) -> Option<String> {
    let text = raw.trim();
    let text = text.strip_prefix("```json").unwrap_or(text);
    let text = text.strip_prefix("```").unwrap_or(text);
    let text = text.strip_suffix("```").unwrap_or(text).trim();
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    let summary = v["summary"].as_str()?.trim().to_string();
    if summary.is_empty() { return None; }
    Some(format!("Summary:\n{summary}"))
}

/// Strip the `<analysis>` scratchpad and unwrap `<summary>` tags.
/// Mirrors CC's `formatCompactSummary`.
pub fn format_compact_summary(raw: &str) -> String {
    // Strip <analysis>…</analysis> (simple state-machine, no regex dep)
    let stripped = strip_xml_tag(raw, "analysis");

    // Extract <summary>…</summary> content
    let formatted = if let Some(content) = extract_xml_tag(&stripped, "summary") {
        format!("Summary:\n{}", content.trim())
    } else {
        stripped.trim().to_string()
    };

    // Collapse 3+ consecutive newlines to 2
    collapse_blank_lines(&formatted)
}

fn strip_xml_tag(s: &str, tag: &str) -> String {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let mut result = s.to_string();
    loop {
        if let Some(start) = result.find(&open) {
            if let Some(rel_end) = result[start..].find(&close) {
                let end = start + rel_end + close.len();
                result.replace_range(start..end, "");
            } else {
                break;
            }
        } else {
            break;
        }
    }
    result
}

fn extract_xml_tag<'a>(s: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = s.find(&open)? + open.len();
    let end = s[start..].find(&close).map(|i| start + i)?;
    Some(&s[start..end])
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
/// context window.  Mirrors CC's `getCompactUserSummaryMessage`.
pub fn compact_summary_to_system_section(summary: &str) -> String {
    format!(
        "\n\n## 对话摘要（早期上下文已压缩）\n\
本对话从一个已达到上下文上限的先前会话延续。以下摘要涵盖了早期对话部分。\n\
\n\
{summary}\n\
\n\
继续从中断处继续工作，无需向用户询问任何问题。直接恢复 — 不要确认摘要，不要重述发生了什么。"
    )
}

// ── Rough token estimation ────────────────────────────────────────────────────

/// Very rough UTF-8 char / 4 token estimate — good enough for threshold checks.
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

/// Check whether messages are above the compact trigger threshold.
pub fn should_compact(messages: &[Message]) -> bool {
    rough_token_count(messages) >= COMPACT_TRIGGER_TOKENS
}

/// Strip images from messages before sending for compaction (saves tokens,
/// avoids hitting compact model's own context limit).
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
/// 1. Sends full history to cheap model with compact system prompt.
/// 2. Formats the raw output (strips `<analysis>`).
/// 3. Persists the summary in `conversations.compact_summary`.
/// 4. Emits a `{"type":"compact","summary":"..."}` SSE event.
/// 5. Returns the formatted summary so the caller can inject it into the
///    system prompt for this generation turn.
///
/// On any failure the function logs a warning and returns `None` — the caller
/// falls back to plain truncation.
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

    if raw.contains("<function_calls>") || raw.contains("<｜DSML｜") {
        warn!(conversation = %ctx.conversation_id, "compact output looks like a tool call, discarding");
        return None;
    }

    let formatted = parse_compact_json(&raw).unwrap_or_else(|| format_compact_summary(&raw));

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
