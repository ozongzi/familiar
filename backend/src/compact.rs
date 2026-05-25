//! Conversation compaction — in-band summaries with boundary-suffix loading.
//!
//! Compaction happens *inside* the conversation, with the same system prompt
//! and the same tools as a normal turn, so the prompt cache keeps hitting and
//! the model can still act. When the provider-reported context crosses the
//! trigger, the worker injects a plain (visible) user message asking for a
//! structured summary; the model's reply is an ordinary assistant message that
//! becomes the condensed earlier context. A second plain user message
//! ("continue answering") bridges back to the user's request. memory + plan
//! ("reminder") is appended to that summary message and is the only in-band
//! place it lives besides the initial snapshot at conversation start — never
//! in the system prompt, so the cached prefix stays byte-stable.
//!
//! # DB schema
//!
//! `conversations.compact_drop_through_msg_id` — the last message folded into
//!                             the latest summary message. Loading returns
//!                             `messages[id > boundary]`. NULL = never
//!                             compacted.
//! `messages.context_tokens` — provider-reported input-context size on the
//!                             latest assistant message, used only to decide
//!                             whether to trigger compaction.
//!
//! # Loading policy
//!
//! [`load_for_generation`] is **boundary-suffix**: it loads the active branch
//! from just past `compact_drop_through_msg_id` onward, i.e.
//! `[recent raw tail, summary(+reminder), …continuation]`. The summary is a
//! real assistant message in the branch, not a synthetic injection, so no
//! token estimate is involved — the load decision is the same boundary the
//! trigger set. A NULL boundary loads the full active branch.

use agentix::Message;
use anyhow::Context;
use sqlx::PgPool;
use tracing::info;
use uuid::Uuid;

use crate::config::ModelConfig;
use crate::db::{Db, MessageRow};
use crate::worker::WorkerContext;

// ── Compact prompt (injected as a plain user turn) ──────────────────────────────
//
// Per-model thresholds live on `ModelConfig`:
//   - compact_trigger_tokens: context_tokens at which a compaction fires
//   - compact_tail_tokens:    recent raw tail kept before the summary

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

// ── Public API: unified loader ────────────────────────────────────────────────

/// Load the message history the worker should feed to the LLM.
///
/// `messages[N..]`: load the active branch starting just after the last
/// compaction boundary. The in-conversation summary message lives just past
/// that boundary and carries the condensed earlier context, so the window is
/// `[recent raw tail, summary(+reminder), …continuation]`. A NULL boundary
/// (never compacted) loads the full active branch; the worker's token-budget
/// truncation guards against overflow either way.
pub async fn load_for_generation(
    db: &Db,
    conversation_id: Uuid,
    user_id: Uuid,
) -> anyhow::Result<Vec<Message>> {
    let drop_through: Option<i64> =
        sqlx::query_scalar("SELECT compact_drop_through_msg_id FROM conversations WHERE id = $1")
            .bind(conversation_id)
            .fetch_optional(&db.pool)
            .await?
            .flatten();
    let (_rows, messages) = db
        .restore_after_rows(conversation_id, user_id, drop_through)
        .await?;
    Ok(messages)
}

// ── Public API: in-band compaction ─────────────────────────────────────────────

/// The summary instruction, injected as a plain (visible) user turn. The
/// system prompt and tools stay in place, so this rides the normal generation
/// loop (cache stays warm; the model may still act) and its reply is an
/// ordinary assistant message.
pub fn summarize_trigger_text() -> String {
    COMPACT_PROMPT.to_string()
}

/// The user turn injected after the summary: the memory + plan reminder
/// followed by the bridge instruction that resumes the user's request. Kept as
/// a plain user message (not appended to the assistant summary) so it rides the
/// same role-alternation as a normal turn and, carrying no "[… UTC]" timestamp,
/// is dropped from public shares. memory + plan lives here and on the initial
/// snapshot only — never in the system prompt.
pub async fn continue_message(ctx: &WorkerContext) -> String {
    const BRIDGE: &str = "上下文已压缩完毕（上一条助手消息即摘要）。现在直接继续回答用户最近的请求，无需确认摘要、不要重述发生了什么。";
    match build_reminder(ctx).await {
        Some(reminder) => format!("{reminder}\n\n{BRIDGE}"),
        None => BRIDGE.to_string(),
    }
}

/// True when the latest provider-reported context on the active branch has
/// crossed the model's trigger. Cheap DB read; call at the worker turn start.
pub async fn should_compact(pool: &PgPool, conv_id: Uuid, model: &ModelConfig) -> bool {
    latest_context_tokens(pool, conv_id).await >= model.compact_trigger_tokens
}

/// Finalise an in-band compaction once the summary turn has been produced:
/// record the drop-through boundary so future loads serve `messages[N..]`.
/// `trigger_msg_id` is the summarise trigger we injected; the kept raw tail is
/// `compact_tail_tokens` of conversation just before it. Emits a `compact`
/// event. The memory + plan reminder is carried by the following continue user
/// message (see [`continue_message`]), not stored here.
pub async fn finalize_compaction(
    ctx: &WorkerContext,
    model: &ModelConfig,
    trigger_msg_id: i64,
) -> anyhow::Result<()> {
    let (rows, messages) = ctx
        .db
        .restore_after_rows(ctx.conversation_id, ctx.user_id, None)
        .await
        .context("restore branch for boundary")?;
    if let Some(drop_through) =
        compute_drop_boundary(&rows, &messages, trigger_msg_id, model.compact_tail_tokens)
    {
        sqlx::query("UPDATE conversations SET compact_drop_through_msg_id = $2 WHERE id = $1")
            .bind(ctx.conversation_id)
            .bind(drop_through)
            .execute(&ctx.pool)
            .await
            .context("record compaction boundary")?;
        info!(conversation = %ctx.conversation_id, drop_through, "⚡ in-band compaction done");
    } else {
        info!(conversation = %ctx.conversation_id, "⚡ in-band compaction done (kept full tail)");
    }

    crate::spells::consolidate_conversation_memories(&ctx.pool, ctx.user_id, ctx.conversation_id)
        .await;
    crate::worker::emit(ctx, serde_json::json!({"type": "compact"})).await;
    Ok(())
}

/// Walk the real conversation (rows whose id precedes the injected trigger)
/// back from the end by `tail_budget_tokens`, snap forward to a User message,
/// and return the id of the last message to drop (everything kept has a larger
/// id). `None` keeps the whole branch (tail budget covers it).
fn compute_drop_boundary(
    rows: &[MessageRow],
    messages: &[Message],
    trigger_msg_id: i64,
    tail_budget_tokens: i64,
) -> Option<i64> {
    let pre: Vec<usize> = (0..rows.len())
        .filter(|&i| rows[i].id < trigger_msg_id)
        .collect();
    if pre.is_empty() {
        return None;
    }

    let mut acc: i64 = 0;
    let mut first_kept = 0usize; // index within `pre`
    for (k, &i) in pre.iter().enumerate().rev() {
        acc += messages[i].estimate_tokens() as i64;
        if acc >= tail_budget_tokens {
            first_kept = k;
            break;
        }
    }
    if first_kept == 0 {
        return None; // whole conversation fits the tail budget → drop nothing
    }

    // Snap forward to a User message so the kept tail never starts mid-turn.
    while first_kept < pre.len() && !matches!(messages[pre[first_kept]], Message::User(_)) {
        first_kept += 1;
    }
    if first_kept == 0 {
        return None;
    }
    let last_dropped = pre[first_kept - 1];
    Some(rows[last_dropped].id)
}

/// memory + plan reminder, or None when neither exists. Plain text (no hidden
/// prefix) — it is appended to the visible summary message.
async fn build_reminder(ctx: &WorkerContext) -> Option<String> {
    let memory =
        crate::spells::load_memories_for_prompt(&ctx.pool, ctx.user_id, ctx.conversation_id).await;

    let plan_row: Option<(String, String)> = sqlx::query_as(
        "SELECT title, steps_json FROM conversation_plans WHERE conversation_id = $1",
    )
    .bind(ctx.conversation_id)
    .fetch_optional(&ctx.pool)
    .await
    .unwrap_or(None);

    if memory.is_none() && plan_row.is_none() {
        return None;
    }

    let mut sections = Vec::new();
    if let Some(memory) = memory {
        sections.push(format!("## 记忆\n{}", memory.trim()));
    }
    if let Some((title, steps_json)) = plan_row {
        sections.push(format!(
            "## 当前计划\n标题：{title}\n步骤 JSON：{steps_json}"
        ));
    }
    Some(format!(
        "---\n（以下为持久背景，自然地当作已知信息使用，不要提及此机制）\n\n{}",
        sections.join("\n\n")
    ))
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

        let _ctx = WorkerContext {
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

        let needs_compact = should_compact(&pool, conv_id, &model).await;
        assert!(
            !needs_compact,
            "inactive branch context must not trigger active branch compaction"
        );
        assert_eq!(
            mock_hits.load(Ordering::SeqCst),
            0,
            "deciding the trigger must not make any LLM call"
        );

        let active_context = latest_context_tokens(&pool, conv_id).await;
        assert_eq!(
            active_context, 100,
            "latest context snapshot should come from the active branch"
        );

        test_db.cleanup().await;
    }
}

