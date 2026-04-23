//! Persistent conversation history backed by PostgreSQL + pgvector.
//!
//! # Schema (see migrations/001_init.sql)
//!
//! messages        — one row per Message, append-only
//! content_tsv     — generated tsvector column for FTS
//! embedding       — vector(1536) column for semantic search
//!
//! # Design
//!
//! - All queries use sqlx with a PgPool — fully async, no blocking threads.
//! - Embeddings are stored as pgvector `vector(1536)` and queried with
//!   cosine distance operator `<=>`.
//! - FTS uses PostgreSQL's built-in `tsvector` / `tsquery` via `@@`.
//! - Summary is stored in conversations.compact_summary, not in messages.
//!   restore() returns all real messages; the worker prepends the summary.

use std::sync::Arc;

use base64::Engine as _;
use pgvector::Vector;
use sqlx::PgPool;
use uuid::Uuid;

use agentix::request::{
    Content, ImageContent, ImageData, Message, ToolCall as AgentToolCall, UserContent,
};

use crate::sandbox::SandboxManager;

// ── Row type ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, sqlx::FromRow)]
#[allow(dead_code)]
pub struct MessageRow {
    pub id: i64,
    pub conversation_id: Uuid,
    pub role: String,
    pub name: Option<String>,
    pub content: Option<String>,
    pub spell_casts: Option<String>,
    pub spell_cast_id: Option<String>,
    pub reasoning: Option<String>,
    pub created_at: i64,
    pub parent_id: Option<i64>,
    pub streaming: bool,
    pub job_id: Option<Uuid>,
    /// Non-null when this message is a compact summary anchor: it means
    /// "the path root..this message has been condensed into `summary_text`".
    /// Any branch whose ancestor chain includes this message can reuse the
    /// summary; branches that don't traverse it fall back to raw history.
    pub summary_text: Option<String>,
    pub summary_tokens: Option<i32>,
}

/// Provider-reported token counts for a single assistant message.
#[derive(Debug, Clone, Copy)]
pub struct MessageTokens {
    pub prompt: i64,
    pub completion: i64,
    pub cache_read: i64,
    pub cache_creation: i64,
}

impl MessageTokens {
    /// Build from an `agentix::UsageStats`.  Returns `None` if no tokens were
    /// recorded (e.g., stream failed before any Usage event arrived).
    pub fn from_usage(u: &agentix::types::UsageStats) -> Option<Self> {
        if u.prompt_tokens == 0 && u.completion_tokens == 0 {
            return None;
        }
        Some(Self {
            prompt: u.prompt_tokens as i64,
            completion: u.completion_tokens as i64,
            cache_read: u.cache_read_tokens as i64,
            cache_creation: u.cache_creation_tokens as i64,
        })
    }
}

// ── Db handle ─────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct Db {
    pub pool: PgPool,
    sandbox: Arc<SandboxManager>,
}

impl Db {
    pub fn new(pool: PgPool, sandbox: Arc<SandboxManager>) -> Self {
        Self { pool, sandbox }
    }

    pub async fn append_streaming(
        &self,
        conversation_id: Uuid,
        job_id: Uuid,
    ) -> anyhow::Result<i64> {
        let now = unix_now();
        let mut tx = self.pool.begin().await?;

        let parent_id: Option<i64> =
            sqlx::query_scalar("SELECT active_message_id FROM conversations WHERE id = $1")
                .bind(conversation_id)
                .fetch_optional(&mut *tx)
                .await?
                .flatten();

        let row_id: i64 = sqlx::query_scalar(
            r#"
            INSERT INTO messages
                (conversation_id, role, content, reasoning, spell_casts, spell_cast_id,
                 name, created_at, streaming, job_id, parent_id)
            VALUES ($1, 'assistant', '', '', NULL, NULL, NULL, $2, true, $3, $4)
            RETURNING id
            "#,
        )
        .bind(conversation_id)
        .bind(now)
        .bind(job_id)
        .bind(parent_id)
        .fetch_one(&mut *tx)
        .await?;

        sqlx::query("UPDATE conversations SET active_message_id = $1 WHERE id = $2")
            .bind(row_id)
            .bind(conversation_id)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        Ok(row_id)
    }

    pub async fn update_streaming_content(
        &self,
        message_id: i64,
        content: &str,
        reasoning: &str,
    ) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE messages SET content = $2, reasoning = $3 WHERE id = $1 AND streaming = true",
        )
        .bind(message_id)
        .bind(content)
        .bind(reasoning)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn seal_streaming_message(
        &self,
        message_id: i64,
        content: Option<&str>,
        reasoning: Option<&str>,
        tool_calls_json: Option<&str>,
        tokens: Option<MessageTokens>,
    ) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE messages
             SET streaming             = false,
                 content               = COALESCE($2, content),
                 reasoning             = COALESCE($3, reasoning),
                 spell_casts           = $4,
                 prompt_tokens         = $5,
                 completion_tokens     = $6,
                 cache_read_tokens     = $7,
                 cache_creation_tokens = $8
             WHERE id = $1",
        )
        .bind(message_id)
        .bind(content)
        .bind(reasoning)
        .bind(tool_calls_json)
        .bind(tokens.map(|t| t.prompt))
        .bind(tokens.map(|t| t.completion))
        .bind(tokens.map(|t| t.cache_read))
        .bind(tokens.map(|t| t.cache_creation))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn append(
        &self,
        conversation_id: Uuid,
        user_id: Uuid,
        msg: &Message,
        embedding: Option<Vector>,
    ) -> anyhow::Result<i64> {
        let (role, content, tool_calls_json, tool_call_id, reasoning): (
            &str,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
        ) = match msg {
            Message::User(parts) => {
                let has_images = parts.iter().any(|p| matches!(p, UserContent::Image(_)));
                let content = if has_images {
                    // Save any inline base64 images to the sandbox and replace
                    // with a __sandbox__:<filename> reference so the DB row
                    // stays small.
                    let mut new_parts: Vec<UserContent> = Vec::with_capacity(parts.len());
                    let conv_dir = self.sandbox.get_conversation_dir(user_id, conversation_id);
                    let _ = tokio::fs::create_dir_all(&conv_dir).await;
                    for part in parts.iter() {
                        match part {
                            UserContent::Image(img) => {
                                if let ImageData::Base64(b64) = &img.data {
                                    let ext = ext_from_mime(&img.mime_type);
                                    let filename = format!("img-{}.{}", Uuid::new_v4(), ext);
                                    let path = conv_dir.join(&filename);
                                    match base64::engine::general_purpose::STANDARD
                                        .decode(b64.as_bytes())
                                    {
                                        Ok(bytes)
                                            if tokio::fs::write(&path, &bytes).await.is_ok() =>
                                        {
                                            new_parts.push(UserContent::Image(ImageContent {
                                                data: ImageData::Url(format!(
                                                    "__sandbox__:{}",
                                                    filename
                                                )),
                                                mime_type: img.mime_type.clone(),
                                            }));
                                        }
                                        // Fallback: keep inline if write fails
                                        _ => new_parts.push(part.clone()),
                                    }
                                } else {
                                    new_parts.push(part.clone());
                                }
                            }
                            _ => new_parts.push(part.clone()),
                        }
                    }
                    let json = serde_json::to_string(&new_parts).unwrap_or_default();
                    format!("__multimodal__:{json}")
                } else {
                    parts
                        .iter()
                        .filter_map(|p| match p {
                            UserContent::Text { text: t } => Some(t.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("")
                };
                ("user", Some(content), None, None, None)
            }
            Message::Assistant {
                content,
                reasoning,
                tool_calls,
            } => {
                let tc_json = if tool_calls.is_empty() {
                    None
                } else {
                    serde_json::to_string(tool_calls).ok()
                };
                (
                    "assistant",
                    content.clone(),
                    tc_json,
                    None,
                    reasoning.clone(),
                )
            }
            Message::ToolResult { call_id, content } => {
                let has_images = content.iter().any(|p| matches!(p, Content::Image(_)));
                let serialized = if has_images {
                    // Offload base64 images to the sandbox. Use MD5 of bytes as
                    // filename so the path matches what generate_image_spell wrote.
                    let conv_dir = self.sandbox.get_conversation_dir(user_id, conversation_id);
                    let pub_dir = conv_dir.join("public");
                    let _ = tokio::fs::create_dir_all(&pub_dir).await;
                    let mut new_content: Vec<Content> = Vec::with_capacity(content.len());
                    for part in content.iter() {
                        match part {
                            Content::Image(img) => {
                                if let ImageData::Base64(b64) = &img.data {
                                    let ext = ext_from_mime(&img.mime_type);
                                    match base64::engine::general_purpose::STANDARD
                                        .decode(b64.as_bytes())
                                    {
                                        Ok(bytes) => {
                                            let hash = format!("{:x}", md5::compute(&bytes));
                                            let filename = format!("img-{}.{}", hash, ext);
                                            let path = pub_dir.join(&filename);
                                            // Write only if not already present (same bytes = same hash).
                                            if !path.exists() {
                                                let _ = tokio::fs::write(&path, &bytes).await;
                                            }
                                            new_content.push(Content::Image(ImageContent {
                                                data: ImageData::Url(format!(
                                                    "__sandbox__:public/{}",
                                                    filename
                                                )),
                                                mime_type: img.mime_type.clone(),
                                            }));
                                        }
                                        Err(_) => new_content.push(part.clone()),
                                    }
                                } else {
                                    new_content.push(part.clone());
                                }
                            }
                            _ => new_content.push(part.clone()),
                        }
                    }
                    serde_json::to_string(&new_content).unwrap_or_default()
                } else {
                    serde_json::to_string(content).unwrap_or_default()
                };
                ("tool", Some(serialized), None, Some(call_id.clone()), None)
            }
        };
        let now = unix_now();

        let mut tx = self.pool.begin().await?;

        let parent_id: Option<i64> =
            sqlx::query_scalar("SELECT active_message_id FROM conversations WHERE id = $1")
                .bind(conversation_id)
                .fetch_optional(&mut *tx)
                .await?
                .flatten();

        let row_id: i64 = sqlx::query_scalar(
            r#"
            INSERT INTO messages
                (conversation_id, role, name, content, spell_casts, spell_cast_id,
                 reasoning, created_at, embedding, parent_id, streaming, job_id)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, false, NULL)
            RETURNING id
            "#,
        )
        .bind(conversation_id)
        .bind(role)
        .bind(Option::<String>::None)
        .bind(&content)
        .bind(&tool_calls_json)
        .bind(&tool_call_id)
        .bind(&reasoning)
        .bind(now)
        .bind(&embedding)
        .bind(parent_id)
        .fetch_one(&mut *tx)
        .await?;

        sqlx::query("UPDATE conversations SET active_message_id = $1 WHERE id = $2")
            .bind(row_id)
            .bind(conversation_id)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;

        Ok(row_id)
    }

    pub async fn branch(&self, conversation_id: Uuid, message_id: i64) -> anyhow::Result<()> {
        sqlx::query("UPDATE conversations SET active_message_id = $1 WHERE id = $2")
            .bind(message_id)
            .bind(conversation_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Descend from `message_id` to its deepest descendant, always picking
    /// the highest-id child (most recent). Returns `message_id` itself if it
    /// has no children.
    pub async fn walk_to_leaf(&self, message_id: i64) -> anyhow::Result<i64> {
        let leaf: Option<i64> = sqlx::query_scalar(
            r#"
            WITH RECURSIVE descent AS (
                SELECT id, 0 AS depth FROM messages WHERE id = $1
                UNION ALL
                SELECT child.id, d.depth + 1
                FROM descent d
                CROSS JOIN LATERAL (
                    SELECT id FROM messages
                    WHERE parent_id = d.id
                    ORDER BY id DESC
                    LIMIT 1
                ) child
            )
            SELECT id FROM descent ORDER BY depth DESC LIMIT 1
            "#,
        )
        .bind(message_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(leaf.unwrap_or(message_id))
    }

    /// Point the conversation at a specific subtree. Walks `message_id`
    /// to its deepest descendant and sets that as `active_message_id`.
    /// Returns the new active leaf id.
    pub async fn activate(&self, conversation_id: Uuid, message_id: i64) -> anyhow::Result<i64> {
        let leaf = self.walk_to_leaf(message_id).await?;
        sqlx::query("UPDATE conversations SET active_message_id = $1 WHERE id = $2")
            .bind(leaf)
            .bind(conversation_id)
            .execute(&self.pool)
            .await?;
        Ok(leaf)
    }

    /// Persist a compact summary anchored on a specific message.
    pub async fn set_summary(
        &self,
        message_id: i64,
        summary: &str,
        tokens: i32,
    ) -> anyhow::Result<()> {
        sqlx::query("UPDATE messages SET summary_text = $1, summary_tokens = $2 WHERE id = $3")
            .bind(summary)
            .bind(tokens)
            .bind(message_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// For each message on the active branch, return its sibling ids
    /// (messages sharing the same `parent_id` in this conversation, in id
    /// order, including the message itself). Used by the UI to render a
    /// `‹ 2/3 ›` branch switcher on messages that have alternatives.
    pub async fn list_active_with_siblings(
        &self,
        conversation_id: Uuid,
    ) -> anyhow::Result<Vec<(MessageRow, Vec<i64>)>> {
        // Single round-trip: walk the active branch + compute siblings per row.
        let rows: Vec<(MessageRow, Vec<i64>)> = sqlx::query_as::<_, SiblingRow>(
            r#"
            WITH RECURSIVE branch AS (
                SELECT m.id, m.conversation_id, m.role, m.name, m.content,
                       m.spell_casts, m.spell_cast_id, m.reasoning,
                       m.created_at, m.parent_id, m.streaming, m.job_id,
                       m.summary_text, m.summary_tokens
                FROM messages m
                JOIN conversations c ON c.id = $1
                WHERE m.id = c.active_message_id
                UNION ALL
                SELECT m.id, m.conversation_id, m.role, m.name, m.content,
                       m.spell_casts, m.spell_cast_id, m.reasoning,
                       m.created_at, m.parent_id, m.streaming, m.job_id,
                       m.summary_text, m.summary_tokens
                FROM messages m
                JOIN branch b ON m.id = b.parent_id
            )
            SELECT b.id, b.conversation_id, b.role, b.name, b.content,
                   b.spell_casts, b.spell_cast_id, b.reasoning, b.created_at,
                   b.parent_id, b.streaming, b.job_id,
                   b.summary_text, b.summary_tokens,
                   COALESCE(
                       (SELECT array_agg(s.id ORDER BY s.id ASC)
                        FROM messages s
                        WHERE s.conversation_id = b.conversation_id
                          AND s.parent_id IS NOT DISTINCT FROM b.parent_id),
                       ARRAY[b.id]
                   ) AS siblings
            FROM branch b
            ORDER BY b.id ASC
            "#,
        )
        .bind(conversation_id)
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(|r| {
            (
                MessageRow {
                    id: r.id,
                    conversation_id: r.conversation_id,
                    role: r.role,
                    name: r.name,
                    content: r.content,
                    spell_casts: r.spell_casts,
                    spell_cast_id: r.spell_cast_id,
                    reasoning: r.reasoning,
                    created_at: r.created_at,
                    parent_id: r.parent_id,
                    streaming: r.streaming,
                    job_id: r.job_id,
                    summary_text: r.summary_text,
                    summary_tokens: r.summary_tokens,
                },
                r.siblings,
            )
        })
        .collect();

        Ok(rows)
    }

    pub async fn set_embedding(&self, row_id: i64, embedding: Vector) -> anyhow::Result<()> {
        sqlx::query("UPDATE messages SET embedding = $1 WHERE id = $2")
            .bind(&embedding)
            .bind(row_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Active-branch history rows paired with their converted `Message`s,
    /// so callers (e.g. `compact.rs`) can inspect per-message `id`,
    /// `summary_text`, etc. alongside the LLM-facing history.
    ///
    /// Filters: streaming rows are excluded; assistant rows whose content,
    /// reasoning, and tool calls are all empty are excluded. Pass
    /// `after_msg_id = Some(id)` to restrict
    /// to rows with `id > after_msg_id` on the active branch.
    pub async fn restore_after_rows(
        &self,
        conversation_id: Uuid,
        user_id: Uuid,
        after_msg_id: Option<i64>,
    ) -> anyhow::Result<(Vec<MessageRow>, Vec<Message>)> {
        let rows = self
            .load_active_rows_filtered(conversation_id, after_msg_id)
            .await?;
        let mut messages: Vec<Message> = rows.iter().cloned().map(row_to_message).collect();
        resolve_sandbox_images(&mut messages, &self.sandbox, user_id, conversation_id).await;
        Ok((rows, messages))
    }

    async fn load_active_rows_filtered(
        &self,
        conversation_id: Uuid,
        after_msg_id: Option<i64>,
    ) -> anyhow::Result<Vec<MessageRow>> {
        let after = after_msg_id.unwrap_or(i64::MIN);
        let rows: Vec<MessageRow> = sqlx::query_as(
            r#"
            WITH RECURSIVE branch AS (
                SELECT m.id, m.conversation_id, m.role, m.name, m.content,
                       m.spell_casts, m.spell_cast_id, m.reasoning,
                       m.created_at, m.parent_id, m.streaming, m.job_id,
                       m.summary_text, m.summary_tokens
                FROM messages m
                JOIN conversations c ON c.id = $1
                WHERE m.id = c.active_message_id AND m.id > $2
                UNION ALL
                SELECT m.id, m.conversation_id, m.role, m.name, m.content,
                       m.spell_casts, m.spell_cast_id, m.reasoning,
                       m.created_at, m.parent_id, m.streaming, m.job_id,
                       m.summary_text, m.summary_tokens
                FROM messages m
                JOIN branch b ON m.id = b.parent_id
                WHERE m.id > $2
            )
            SELECT id, conversation_id, role, name, content,
                   spell_casts, spell_cast_id, reasoning, created_at, parent_id,
                   streaming, job_id, summary_text, summary_tokens
            FROM branch
            WHERE streaming = false
              AND NOT (role = 'assistant'
                       AND (content IS NULL OR content = '')
                       AND (reasoning IS NULL OR reasoning = '')
                       AND spell_casts IS NULL)
            ORDER BY id ASC
            "#,
        )
        .bind(conversation_id)
        .bind(after)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn fts_search(
        &self,
        conversation_id: Uuid,
        query: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<MessageRow>> {
        let rows: Vec<MessageRow> = sqlx::query_as(
            r#"
            SELECT id, conversation_id, role, name, content,
                   spell_casts, spell_cast_id, reasoning, created_at,
                   parent_id, streaming, job_id, summary_text, summary_tokens
            FROM messages
            WHERE conversation_id = $1
              AND content_tsv @@ plainto_tsquery('simple', $2)
            ORDER BY id DESC
            LIMIT $3
            "#,
        )
        .bind(conversation_id)
        .bind(query)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows)
    }

    pub async fn semantic_search(
        &self,
        conversation_id: Uuid,
        query_vec: Vector,
        limit: usize,
    ) -> anyhow::Result<Vec<(MessageRow, f32)>> {
        let rows: Vec<(MessageRow, f32)> = sqlx::query_as(
            r#"
            SELECT id, conversation_id, role, name, content,
                   spell_casts, spell_cast_id, reasoning, created_at,
                   parent_id, streaming, job_id, summary_text, summary_tokens,
                   (1 - (embedding <=> $2))::float4 AS similarity
            FROM messages
            WHERE conversation_id = $1
              AND embedding IS NOT NULL
            ORDER BY embedding <=> $2
            LIMIT $3
            "#,
        )
        .bind(conversation_id)
        .bind(&query_vec)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(|r: SemanticRow| {
            (
                MessageRow {
                    id: r.id,
                    conversation_id: r.conversation_id,
                    role: r.role,
                    name: r.name,
                    content: r.content,
                    spell_casts: r.spell_casts,
                    spell_cast_id: r.spell_cast_id,
                    reasoning: r.reasoning,
                    created_at: r.created_at,
                    parent_id: r.parent_id,
                    streaming: r.streaming,
                    job_id: r.job_id,
                    summary_text: r.summary_text,
                    summary_tokens: r.summary_tokens,
                },
                r.similarity,
            )
        })
        .collect();

        Ok(rows)
    }
}

// ── Internal helpers ──────────────────────────────────────────────────────────

#[derive(sqlx::FromRow)]
struct SiblingRow {
    id: i64,
    conversation_id: Uuid,
    role: String,
    name: Option<String>,
    content: Option<String>,
    spell_casts: Option<String>,
    spell_cast_id: Option<String>,
    reasoning: Option<String>,
    created_at: i64,
    parent_id: Option<i64>,
    streaming: bool,
    job_id: Option<Uuid>,
    summary_text: Option<String>,
    summary_tokens: Option<i32>,
    siblings: Vec<i64>,
}

#[derive(sqlx::FromRow)]
struct SemanticRow {
    id: i64,
    conversation_id: Uuid,
    role: String,
    name: Option<String>,
    content: Option<String>,
    spell_casts: Option<String>,
    spell_cast_id: Option<String>,
    reasoning: Option<String>,
    created_at: i64,
    parent_id: Option<i64>,
    streaming: bool,
    job_id: Option<Uuid>,
    summary_text: Option<String>,
    summary_tokens: Option<i32>,
    similarity: f32,
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

pub fn row_to_message(row: MessageRow) -> Message {
    match row.role.as_str() {
        "tool" => Message::ToolResult {
            call_id: row.spell_cast_id.unwrap_or_default(),
            content: {
                let s = row.content.unwrap_or_default();
                serde_json::from_str(&s).unwrap_or_else(|_| vec![agentix::Content::text(s)])
            },
        },
        "assistant" => {
            let tool_calls: Vec<AgentToolCall> = row
                .spell_casts
                .as_deref()
                .and_then(|s| serde_json::from_str(s).ok())
                .unwrap_or_default();
            // Anthropic requires tool_use.input to be a JSON object.
            // Guard against two cases that cause HTTP 400 on replay:
            //   1. Truncated/invalid JSON (stream cut off mid-write) → parse fails
            //   2. LLM generated a bare array instead of {"param": [...]}
            let tool_calls = tool_calls
                .into_iter()
                .map(|mut tc| {
                    let fixed = match serde_json::from_str::<serde_json::Value>(&tc.arguments) {
                        Ok(val) if val.is_object() => return tc, // already valid
                        Ok(val) => match tc.name.as_str() {
                            "multiwrite" if val.is_array() => serde_json::json!({ "writes": val }),
                            "multiread" if val.is_array() => serde_json::json!({ "reads": val }),
                            _ => serde_json::json!({}),
                        },
                        Err(_) => serde_json::json!({}), // truncated / invalid JSON
                    };
                    tc.arguments =
                        serde_json::to_string(&fixed).unwrap_or_else(|_| "{}".to_string());
                    tc
                })
                .collect();
            Message::Assistant {
                content: row.content,
                reasoning: row.reasoning,
                tool_calls,
            }
        }
        _ => {
            let raw = row.content.unwrap_or_default();
            if let Some(json) = raw.strip_prefix("__multimodal__:") {
                let parts: Vec<UserContent> = serde_json::from_str(json).unwrap_or_else(|_| {
                    vec![UserContent::Text {
                        text: raw.to_string(),
                    }]
                });
                Message::User(parts)
            } else {
                Message::User(vec![UserContent::Text { text: raw }])
            }
        }
    }
}

pub fn to_vector(v: Vec<f32>) -> Vector {
    Vector::from(v)
}

/// Detect image MIME type from magic bytes. Returns `None` for non-image data.
fn mime_from_bytes(data: &[u8]) -> Option<&'static str> {
    if data.len() >= 3 && data[0] == 0xFF && data[1] == 0xD8 && data[2] == 0xFF {
        return Some("image/jpeg");
    }
    if data.len() >= 8
        && data[0] == 0x89
        && &data[1..4] == b"PNG"
        && data[4] == 0x0D
        && data[5] == 0x0A
        && data[6] == 0x1A
        && data[7] == 0x0A
    {
        return Some("image/png");
    }
    if data.len() >= 6 && (&data[..6] == b"GIF87a" || &data[..6] == b"GIF89a") {
        return Some("image/gif");
    }
    if data.len() >= 12 && &data[0..4] == b"RIFF" && &data[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    None
}

/// Map a MIME type to a short file extension for sandbox image files.
fn ext_from_mime(mime: &str) -> &'static str {
    match mime {
        "image/jpeg" | "image/jpg" => "jpg",
        "image/png" => "png",
        "image/gif" => "gif",
        "image/webp" => "webp",
        _ => "bin",
    }
}

/// Resolve `ImageData::Url("__sandbox__:<filename>")` references in messages
/// loaded from the DB: read the file from the conversation sandbox directory
/// and replace with `ImageData::Base64`.
///
/// Old messages that already store inline `ImageData::Base64` are left as-is.
async fn resolve_sandbox_images(
    messages: &mut Vec<Message>,
    sandbox: &SandboxManager,
    user_id: Uuid,
    conversation_id: Uuid,
) {
    let conv_dir = sandbox.get_conversation_dir(user_id, conversation_id);

    // Find the last image part across all messages — any format (sandbox URL
    // or inline base64). Only that one is kept; all earlier images are
    // replaced with a text note so the model knows where to find them.
    let last_img_pos: Option<(usize, usize)> = messages
        .iter()
        .enumerate()
        .flat_map(|(mi, msg)| {
            let parts: &[UserContent] = match msg {
                Message::User(p) => p,
                Message::ToolResult { content, .. } => content,
                _ => &[],
            };
            parts.iter().enumerate().filter_map(move |(pi, p)| {
                if matches!(p, UserContent::Image(_)) {
                    Some((mi, pi))
                } else {
                    None
                }
            })
        })
        .last();

    let Some((keep_mi, keep_pi)) = last_img_pos else {
        return;
    };

    for (mi, msg) in messages.iter_mut().enumerate() {
        let parts: &mut Vec<UserContent> = match msg {
            Message::User(p) => p,
            Message::ToolResult { content, .. } => content,
            _ => continue,
        };
        for (pi, part) in parts.iter_mut().enumerate() {
            let UserContent::Image(img) = part else {
                continue;
            };

            if mi == keep_mi && pi == keep_pi {
                // Latest image: resolve __sandbox__: → base64 if needed.
                if let ImageData::Url(url) = &img.data {
                    if let Some(filename) = url.strip_prefix("__sandbox__:") {
                        let path = conv_dir.join(filename);
                        match tokio::fs::read(&path).await {
                            Ok(bytes) => {
                                // Re-detect mime type from magic bytes so a
                                // mismatched stored type (e.g. image/png for a
                                // JPEG file) doesn't cause provider 400 errors.
                                if let Some(detected) = mime_from_bytes(&bytes) {
                                    img.mime_type = detected.to_string();
                                }
                                img.data = ImageData::Base64(
                                    base64::engine::general_purpose::STANDARD.encode(&bytes),
                                );
                            }
                            Err(e) => tracing::warn!(?path, "sandbox image read failed: {e}"),
                        }
                    }
                }
                // Already base64 — leave as-is.
            } else {
                // Older image: replace with a text note giving the sandbox path.
                let sandbox_path = match &img.data {
                    ImageData::Url(u) if u.starts_with("__sandbox__:") => {
                        format!("/workspace/{}", &u["__sandbox__:".len()..])
                    }
                    ImageData::Url(u) => u.clone(),
                    ImageData::Base64(_) => "(inline image)".to_string(),
                };
                *part = UserContent::Text {
                    text: format!("[图片未显示，已存储于 {sandbox_path}，如需查看可使用 read]"),
                };
            }
        }
    }
}
