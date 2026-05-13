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
use anyhow::{Context, anyhow};
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
/// the anchor. Errors if the raw history is over budget and no usable
/// summary exists on the active path — the worker must surface this to
/// the user rather than silently sending an oversized prompt.
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

    Err(anyhow!(
        "对话上下文 {raw_total} tokens 超过预算 {budget}，且没有可用的摘要锚点可以套用。请触发一次压缩或开启新对话。"
    ))
}

// ── Public API: compaction ────────────────────────────────────────────────────

/// Check if the conversation exceeds the compact trigger and, if so, run
/// incremental summarisation + write a new anchor on the boundary message.
/// Raw messages are never deleted: any existing anchor stays valid for
/// branches that still traverse it.
///
/// Returns `Ok(true)` if a new summary was written, `Ok(false)` if the
/// trigger was not met or there was nothing to summarise. Failures during
/// summarisation (LLM call, DB write) are surfaced as `Err` so the worker
/// can show the user a clear error rather than silently continuing with
/// an oversized prompt. A `compact_failed` event is emitted with the
/// underlying error before the `Err` is returned.
pub async fn maybe_compact(
    ctx: &WorkerContext,
    model: &ModelConfig,
    http: &reqwest::Client,
) -> anyhow::Result<bool> {
    let ctx_tokens = latest_context_tokens(&ctx.pool, ctx.conversation_id).await;
    if ctx_tokens < model.compact_trigger_tokens {
        return Ok(false);
    }

    info!(
        conversation = %ctx.conversation_id,
        ctx_tokens,
        "⚡ compaction threshold reached"
    );

    match do_compact(ctx, model, http, ctx_tokens).await {
        Ok(written) => Ok(written),
        Err(e) => {
            let msg = format!("{e:#}");
            warn!(conversation = %ctx.conversation_id, "compaction failed: {msg}");
            crate::worker::emit(
                ctx,
                serde_json::json!({"type": "compact_failed", "error": &msg}),
            )
            .await;
            Err(e.context("conversation compaction failed"))
        }
    }
}

async fn do_compact(
    ctx: &WorkerContext,
    model: &ModelConfig,
    http: &reqwest::Client,
    ctx_tokens: i64,
) -> anyhow::Result<bool> {
    // Full active path + the latest anchor on that path (if any).
    let (rows, messages) = ctx
        .db
        .restore_after_rows(ctx.conversation_id, ctx.user_id, None)
        .await
        .context("restore active branch for compaction")?;

    let prev_anchor_idx = rows.iter().rposition(|r| r.summary_text.is_some());
    let prev_summary = prev_anchor_idx.and_then(|i| rows[i].summary_text.clone());
    let delta_start = prev_anchor_idx.map(|i| i + 1).unwrap_or(0);
    let delta_rows: &[MessageRow] = &rows[delta_start..];
    let delta_msgs: &[Message] = &messages[delta_start..];

    let Some(split_idx) = compute_new_boundary(delta_msgs, model.compact_tail_tokens) else {
        info!(
            conversation = %ctx.conversation_id,
            ctx_tokens,
            delta_len = delta_msgs.len(),
            "nothing to summarise after boundary search"
        );
        return Ok(false);
    };

    let new_until_msg_id = delta_rows[split_idx - 1].id;

    // Build summariser input: [prev_summary?, delta[..split_idx]].
    let mut input: Vec<Message> = Vec::new();
    if let Some(ref s) = prev_summary {
        input.push(summary_to_message(s));
    }
    input.extend_from_slice(&delta_msgs[..split_idx]);

    // Tell the user a compaction is starting — the summariser call can take
    // many seconds and otherwise looks like a stall.
    crate::worker::emit(
        ctx,
        serde_json::json!({
            "type": "compact_started",
            "ctx_tokens": ctx_tokens,
            "trigger_tokens": model.compact_trigger_tokens,
        }),
    )
    .await;

    let new_summary = run_summariser(ctx, model, http, &input).await?;

    let summary_tokens = summary_to_message(&new_summary).estimate_tokens() as i32;

    ctx.db
        .set_summary(new_until_msg_id, &new_summary, summary_tokens)
        .await
        .context("persist new summary anchor")?;

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

    Ok(true)
}

// ── Internal: boundary search ─────────────────────────────────────────────────

/// Latest provider-reported input context size on an assistant message.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Provider;
    use crate::db::Db;
    use crate::sandbox::SandboxManager;
    use crate::worker::WorkerContext;
    use agentix::{Message, UserContent};
    use sqlx::{Executor, PgPool, postgres::PgPoolOptions};
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    struct TestDb {
        pool: PgPool,
        admin_url: String,
        db_name: String,
    }

    impl TestDb {
        async fn cleanup(self) {
            let Self {
                pool,
                admin_url,
                db_name,
            } = self;
            pool.close().await;
            if let Ok(admin) = PgPoolOptions::new()
                .max_connections(1)
                .connect(&admin_url)
                .await
            {
                let _ = admin
                    .execute(format!("DROP DATABASE IF EXISTS \"{db_name}\" WITH (FORCE)").as_str())
                    .await;
            }
        }
    }

    async fn fresh_db() -> TestDb {
        let admin_url = std::env::var("DATABASE_URL_TEST")
            .expect("DATABASE_URL_TEST must be set for integration tests");
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&admin_url)
            .await
            .expect("connect admin pool");
        let db_name = format!("familiar_test_{}", Uuid::new_v4().simple());
        admin
            .execute(format!("CREATE DATABASE \"{db_name}\"").as_str())
            .await
            .expect("create test db");

        let test_url = switch_db(&admin_url, &db_name);
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .connect(&test_url)
            .await
            .expect("connect test pool");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("run migrations");
        TestDb {
            pool,
            admin_url,
            db_name,
        }
    }

    fn switch_db(url: &str, db: &str) -> String {
        let (scheme_host, rest) = url.split_once("://").expect("postgres:// URL");
        let (auth_host, tail) = rest.split_once('/').unwrap_or((rest, ""));
        let query = tail
            .split_once('?')
            .map(|(_, q)| format!("?{q}"))
            .unwrap_or_default();
        format!("{scheme_host}://{auth_host}/{db}{query}")
    }

    async fn seed(pool: &PgPool) -> (Uuid, Uuid, Uuid) {
        let user_id: Uuid = sqlx::query_scalar(
            "INSERT INTO users (name, password_hash) VALUES ($1, $2) RETURNING id",
        )
        .bind(format!("compact-tester-{}", Uuid::new_v4()))
        .bind("$2b$12$dummyhash")
        .fetch_one(pool)
        .await
        .expect("insert user");

        let conv_id: Uuid =
            sqlx::query_scalar("INSERT INTO conversations (user_id) VALUES ($1) RETURNING id")
                .bind(user_id)
                .fetch_one(pool)
                .await
                .expect("insert conversation");

        let job_id: Uuid = sqlx::query_scalar(
            "INSERT INTO generation_jobs (conversation_id, user_id) VALUES ($1, $2) RETURNING id",
        )
        .bind(conv_id)
        .bind(user_id)
        .fetch_one(pool)
        .await
        .expect("insert generation job");

        (user_id, conv_id, job_id)
    }

    async fn start_anthropic_mock() -> (String, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock server");
        let addr = listener.local_addr().expect("mock addr");
        let hits = Arc::new(AtomicUsize::new(0));
        let hits_for_task = Arc::clone(&hits);

        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    break;
                };
                hits_for_task.fetch_add(1, Ordering::SeqCst);
                tokio::spawn(async move {
                    let mut buf = vec![0; 4096];
                    let _ = socket.read(&mut buf).await;
                    let body = concat!(
                        "event: message_start\n",
                        "data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":10,\"output_tokens\":0}}}\n\n",
                        "event: content_block_start\n",
                        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
                        "event: content_block_delta\n",
                        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"mock summary\"}}\n\n",
                        "event: content_block_stop\n",
                        "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
                        "event: message_delta\n",
                        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"input_tokens\":0,\"output_tokens\":2}}\n\n",
                        "event: message_stop\n",
                        "data: {\"type\":\"message_stop\"}\n\n"
                    );
                    let response = format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                });
            }
        });

        (format!("http://{addr}"), hits)
    }

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

    #[derive(Clone, Copy)]
    struct BranchNode {
        id: i64,
        parent_id: Option<i64>,
        is_assistant: bool,
        context_tokens: Option<i64>,
    }

    fn latest_context_from_active_branch(active_id: i64, nodes: &[BranchNode]) -> i64 {
        let mut current = Some(active_id);
        while let Some(id) = current {
            let Some(node) = nodes.iter().find(|n| n.id == id) else {
                break;
            };
            if node.is_assistant
                && let Some(tokens) = node.context_tokens
            {
                return tokens;
            }
            current = node.parent_id;
        }
        0
    }

    #[test]
    fn latest_context_selection_ignores_newer_inactive_branch_rows() {
        let nodes = [
            BranchNode {
                id: 1,
                parent_id: None,
                is_assistant: false,
                context_tokens: None,
            },
            BranchNode {
                id: 2,
                parent_id: Some(1),
                is_assistant: true,
                context_tokens: Some(100),
            },
            BranchNode {
                id: 3,
                parent_id: Some(2),
                is_assistant: false,
                context_tokens: None,
            },
            BranchNode {
                id: 4,
                parent_id: Some(3),
                is_assistant: true,
                context_tokens: None,
            },
            BranchNode {
                id: 5,
                parent_id: Some(4),
                is_assistant: false,
                context_tokens: None,
            },
            BranchNode {
                id: 6,
                parent_id: Some(1),
                is_assistant: false,
                context_tokens: None,
            },
            BranchNode {
                id: 7,
                parent_id: Some(6),
                is_assistant: true,
                context_tokens: Some(5_000),
            },
            BranchNode {
                id: 8,
                parent_id: Some(7),
                is_assistant: false,
                context_tokens: None,
            },
            BranchNode {
                id: 9,
                parent_id: Some(8),
                is_assistant: true,
                context_tokens: Some(9_000),
            },
        ];

        assert_eq!(latest_context_from_active_branch(5, &nodes), 100);
        assert_eq!(latest_context_from_active_branch(9, &nodes), 9_000);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL_TEST pointing at a Postgres server that can CREATE DATABASE"]
    async fn compact_trigger_uses_latest_context_on_active_branch_only() {
        let test_db = fresh_db().await;
        let pool = test_db.pool.clone();
        let sandbox = Arc::new(SandboxManager::new(PathBuf::from(
            "/tmp/familiar-compact-test",
        )));
        let db = Db::new(pool.clone(), Arc::clone(&sandbox));
        let (user_id, conv_id, job_id) = seed(&pool).await;
        let (mock_base, mock_hits) = start_anthropic_mock().await;

        // Active branch: context snapshot is below the trigger, but the raw
        // active path is large enough that the old conversation-wide trigger
        // would have compacted it if polluted by another branch.
        let root = db
            .append(conv_id, user_id, &user_msg("root"), None)
            .await
            .expect("append root");
        let _active_a1 = db
            .append(conv_id, user_id, &assistant_msg("active a1"), None)
            .await
            .expect("append active a1");
        let _active_u2 = db
            .append(
                conv_id,
                user_id,
                &user_msg("active tail ".repeat(300)),
                None,
            )
            .await
            .expect("append active u2");
        let active_leaf = db
            .append(conv_id, user_id, &assistant_msg("active leaf"), None)
            .await
            .expect("append active leaf");
        sqlx::query("UPDATE messages SET context_tokens = 100 WHERE id = $1")
            .bind(active_leaf)
            .execute(&pool)
            .await
            .expect("set active context");

        // Inactive branch has the newest assistant row and a high context
        // snapshot. The regression was that this row triggered compacting the
        // active branch even though it is not on the active path.
        sqlx::query("UPDATE conversations SET active_message_id = $1 WHERE id = $2")
            .bind(root)
            .bind(conv_id)
            .execute(&pool)
            .await
            .expect("rewind to branch root");
        let _inactive_user = db
            .append(conv_id, user_id, &user_msg("inactive branch"), None)
            .await
            .expect("append inactive user");
        let inactive_leaf = db
            .append(
                conv_id,
                user_id,
                &assistant_msg("inactive high context"),
                None,
            )
            .await
            .expect("append inactive leaf");
        sqlx::query("UPDATE messages SET context_tokens = 5_000 WHERE id = $1")
            .bind(inactive_leaf)
            .execute(&pool)
            .await
            .expect("set inactive context");

        sqlx::query("UPDATE conversations SET active_message_id = $1 WHERE id = $2")
            .bind(active_leaf)
            .bind(conv_id)
            .execute(&pool)
            .await
            .expect("restore active branch");

        let ctx = WorkerContext {
            job_id,
            conversation_id: conv_id,
            user_id,
            pool: pool.clone(),
            db,
            sandbox,
            tunnel_registry: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        };
        let model = ModelConfig {
            api_key: "test-key".to_string(),
            api_base: mock_base,
            name: "claude-test".to_string(),
            provider: Provider::Anthropic,
            extra_body: HashMap::new(),
            max_tokens: None,
            kind: "api".to_string(),
            compact_trigger_tokens: 1_000,
            compact_tail_tokens: 100,
            reasoning_effort: None,
        };

        let compacted = maybe_compact(&ctx, &model, &reqwest::Client::new())
            .await
            .expect("maybe_compact should not error when trigger is not met");
        assert!(
            !compacted,
            "inactive branch context must not trigger active branch compaction"
        );
        assert_eq!(
            mock_hits.load(Ordering::SeqCst),
            0,
            "summarizer mock should not be called when only inactive branch is over trigger"
        );

        let summary_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM messages WHERE conversation_id = $1 AND summary_text IS NOT NULL",
        )
        .bind(conv_id)
        .fetch_one(&pool)
        .await
        .expect("count summaries");
        assert_eq!(summary_count, 0, "no summary anchor should be written");

        let active_context = latest_context_tokens(&pool, conv_id).await;
        assert_eq!(
            active_context, 100,
            "latest context snapshot should come from the active branch"
        );

        test_db.cleanup().await;
    }
}

// ── Internal: summariser ──────────────────────────────────────────────────────

async fn run_summariser_http(
    model: &ModelConfig,
    http: &reqwest::Client,
    messages: Vec<Message>,
) -> anyhow::Result<String> {
    let request = model
        .to_request()
        .system_prompt(build_compact_system_prompt())
        .messages(messages)
        .max_tokens(COMPACT_MAX_OUTPUT_TOKENS);

    let mut stream = request
        .stream(http)
        .await
        .map_err(|e| anyhow!("compaction LLM stream failed to start: {e}"))?;

    let mut raw = String::new();
    while let Some(event) = stream.next().await {
        match event {
            LlmEvent::Token(t) => raw.push_str(&t),
            LlmEvent::Error(e) => {
                return Err(anyhow!("compaction LLM stream error: {e}"));
            }
            _ => {}
        }
    }
    Ok(raw)
}

/// Drive `claude -p` (subprocess) with no tools and collect its text output.
/// Used when the frontier model itself is `kind=claude-code` and has no
/// HTTP credentials of its own.
async fn run_summariser_claude_code(
    model: &ModelConfig,
    http: &reqwest::Client,
    mut messages: Vec<Message>,
) -> anyhow::Result<String> {
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
    let mut stream = request
        .stream(http)
        .await
        .map_err(|e| anyhow!("compaction (claude-code) stream failed to start: {e}"))?;

    let mut raw = String::new();
    while let Some(event) = stream.next().await {
        match event {
            LlmEvent::Token(t) => raw.push_str(&t),
            LlmEvent::Error(e) => {
                return Err(anyhow!("compaction (claude-code) stream error: {e}"));
            }
            _ => {}
        }
    }
    Ok(raw)
}

async fn run_summariser(
    ctx: &WorkerContext,
    model: &ModelConfig,
    http: &reqwest::Client,
    input: &[Message],
) -> anyhow::Result<String> {
    let compact_messages = strip_images(input);

    let raw = if model.kind == "claude-code" {
        run_summariser_claude_code(model, http, compact_messages).await?
    } else {
        run_summariser_http(model, http, compact_messages).await?
    };

    if raw.trim().is_empty() {
        return Err(anyhow!(
            "compaction model returned an empty response (conversation={})",
            ctx.conversation_id
        ));
    }

    if strip_tool_call_blocks(&raw).trim().is_empty() {
        return Err(anyhow!(
            "compaction model returned only tool-call blocks (conversation={})",
            ctx.conversation_id
        ));
    }

    let formatted = format_compact_summary(&raw);
    if formatted.trim().is_empty() || formatted == "Summary:" {
        return Err(anyhow!(
            "compaction model produced an empty summary after formatting (conversation={})",
            ctx.conversation_id
        ));
    }
    Ok(formatted)
}
