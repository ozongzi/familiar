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
//! - On restore, we find the latest summary row and load only
//!   [that summary + everything after it] to keep the in-memory history bounded.

use pgvector::Vector;
use sqlx::PgPool;
use uuid::Uuid;

use agentix::request::{Message, ToolCall as AgentToolCall, UserContent};

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
    pub is_summary: bool,
    pub created_at: i64,
    pub parent_id: Option<i64>,
    pub streaming: bool,
    pub job_id: Option<Uuid>,
}

// ── Db handle ─────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct Db {
    pub pool: PgPool,
}

impl Db {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn append_streaming(
        &self,
        conversation_id: Uuid,
        job_id: Uuid,
    ) -> anyhow::Result<i64> {
        let now = unix_now();
        let mut tx = self.pool.begin().await?;

        let parent_id: Option<i64> = sqlx::query_scalar(
            "SELECT active_message_id FROM conversations WHERE id = $1",
        )
        .bind(conversation_id)
        .fetch_optional(&mut *tx)
        .await?
        .flatten();

        let row_id: i64 = sqlx::query_scalar(
            r#"
            INSERT INTO messages
                (conversation_id, role, content, reasoning, spell_casts, spell_cast_id,
                 name, is_summary, created_at, streaming, job_id, parent_id)
            VALUES ($1, 'assistant', '', '', NULL, NULL, NULL, false, $2, true, $3, $4)
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
    ) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE messages
             SET streaming    = false,
                 content      = COALESCE($2, content),
                 reasoning    = COALESCE($3, reasoning),
                 spell_casts  = $4
             WHERE id = $1",
        )
        .bind(message_id)
        .bind(content)
        .bind(reasoning)
        .bind(tool_calls_json)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn seal_all_streaming_for_job(&self, job_id: Uuid) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE messages SET streaming = false WHERE job_id = $1 AND streaming = true",
        )
        .bind(job_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn append(
        &self,
        conversation_id: Uuid,
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
                    let json = serde_json::to_string(parts).unwrap_or_default();
                    format!("__multimodal__:{json}")
                } else {
                    parts
                        .iter()
                        .filter_map(|p| match p {
                            UserContent::Text(t) => Some(t.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("")
                };
                ("user", Some(content), None, None, None)
            }
            Message::Assistant { content, reasoning, tool_calls } => {
                let tc_json = if tool_calls.is_empty() {
                    None
                } else {
                    serde_json::to_string(tool_calls).ok()
                };
                ("assistant", content.clone(), tc_json, None, reasoning.clone())
            }
            Message::ToolResult { call_id, content } => {
                ("tool", Some(content.clone()), None, Some(call_id.clone()), None)
            }
        };
        let is_summary = false;
        let now = unix_now();

        let mut tx = self.pool.begin().await?;

        let parent_id: Option<i64> = sqlx::query_scalar(
            "SELECT active_message_id FROM conversations WHERE id = $1",
        )
        .bind(conversation_id)
        .fetch_optional(&mut *tx)
        .await?
        .flatten();

        let row_id: i64 = sqlx::query_scalar(
            r#"
            INSERT INTO messages
                (conversation_id, role, name, content, spell_casts, spell_cast_id,
                 reasoning, is_summary, created_at, embedding, parent_id, streaming, job_id)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, false, NULL)
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
        .bind(is_summary)
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

    pub async fn set_embedding(&self, row_id: i64, embedding: Vector) -> anyhow::Result<()> {
        sqlx::query("UPDATE messages SET embedding = $1 WHERE id = $2")
            .bind(&embedding)
            .bind(row_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn restore(&self, conversation_id: Uuid) -> anyhow::Result<Vec<Message>> {
        let rows: Vec<MessageRow> = sqlx::query_as(
            r#"
            WITH RECURSIVE branch AS (
                SELECT m.id, m.conversation_id, m.role, m.name, m.content,
                       m.spell_casts, m.spell_cast_id, m.reasoning, m.is_summary,
                       m.created_at, m.parent_id, m.streaming, m.job_id
                FROM messages m
                JOIN conversations c ON c.id = $1
                WHERE m.id = c.active_message_id
                UNION ALL
                SELECT m.id, m.conversation_id, m.role, m.name, m.content,
                       m.spell_casts, m.spell_cast_id, m.reasoning, m.is_summary,
                       m.created_at, m.parent_id, m.streaming, m.job_id
                FROM messages m
                JOIN branch b ON m.id = b.parent_id
            ),
            summary_cutoff AS (
                SELECT COALESCE(
                    (SELECT id FROM branch WHERE is_summary = true ORDER BY id DESC LIMIT 1),
                    0
                ) AS since_id
            )
            SELECT id, conversation_id, role, name, content,
                   spell_casts, spell_cast_id, reasoning, is_summary, created_at, parent_id,
                   streaming, job_id
            FROM branch, summary_cutoff
            WHERE id >= since_id
              AND streaming = false
              AND NOT (role = 'assistant'
                       AND (content IS NULL OR content = '')
                       AND spell_casts IS NULL)
            ORDER BY id ASC
            "#,
        )
        .bind(conversation_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(row_to_message).collect())
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
                   spell_casts, spell_cast_id, reasoning, is_summary, created_at,
                   parent_id, streaming, job_id
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
                   spell_casts, spell_cast_id, reasoning, is_summary, created_at,
                   parent_id, streaming, job_id,
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
                    is_summary: r.is_summary,
                    created_at: r.created_at,
                    parent_id: r.parent_id,
                    streaming: r.streaming,
                    job_id: r.job_id,
                },
                r.similarity,
            )
        })
        .collect();

        Ok(rows)
    }

    pub async fn list_messages(&self, conversation_id: Uuid) -> anyhow::Result<Vec<MessageRow>> {
        let rows: Vec<MessageRow> = sqlx::query_as(
            r#"
            WITH RECURSIVE branch AS (
                SELECT m.id, m.conversation_id, m.role, m.name, m.content,
                       m.spell_casts, m.spell_cast_id, m.reasoning, m.is_summary,
                       m.created_at, m.parent_id, m.streaming, m.job_id
                FROM messages m
                JOIN conversations c ON c.id = $1
                WHERE m.id = c.active_message_id
                UNION ALL
                SELECT m.id, m.conversation_id, m.role, m.name, m.content,
                       m.spell_casts, m.spell_cast_id, m.reasoning, m.is_summary,
                       m.created_at, m.parent_id, m.streaming, m.job_id
                FROM messages m
                JOIN branch b ON m.id = b.parent_id
            )
            SELECT id, conversation_id, role, name, content,
                   spell_casts, spell_cast_id, reasoning, is_summary, created_at, parent_id,
                   streaming, job_id
            FROM branch
            ORDER BY id ASC
            "#,
        )
        .bind(conversation_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows)
    }
}

// ── Internal helpers ──────────────────────────────────────────────────────────

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
    is_summary: bool,
    created_at: i64,
    parent_id: Option<i64>,
    streaming: bool,
    job_id: Option<Uuid>,
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
            content: row.content.unwrap_or_default(),
        },
        "assistant" => {
            let tool_calls: Vec<AgentToolCall> = row
                .spell_casts
                .as_deref()
                .and_then(|s| serde_json::from_str(s).ok())
                .unwrap_or_default();
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
                    vec![UserContent::Text(raw.to_string())]
                });
                Message::User(parts)
            } else {
                Message::User(vec![UserContent::Text(raw)])
            }
        }
    }
}

pub fn to_vector(v: Vec<f32>) -> Vector {
    Vector::from(v)
}
