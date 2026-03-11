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

use ds_api::raw::request::message::{Message, Role};

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
    pub is_summary: bool,
    pub created_at: i64,
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

    // ── Write ─────────────────────────────────────────────────────────────────

    /// Persist a single `Message` for `conversation_id`.
    /// Returns the new row id.
    pub async fn append(
        &self,
        conversation_id: Uuid,
        msg: &Message,
        embedding: Option<Vector>,
    ) -> anyhow::Result<i64> {
        let role = role_to_str(&msg.role);
        let tool_calls = msg
            .tool_calls
            .as_ref()
            .and_then(|tc| serde_json::to_string(tc).ok());
        let is_summary = msg.is_auto_summary();
        let now = unix_now();

        let row_id: i64 = sqlx::query_scalar(
            r#"
            INSERT INTO messages
                (conversation_id, role, name, content, spell_casts, spell_cast_id,
                 is_summary, created_at, embedding)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            RETURNING id
            "#,
        )
        .bind(conversation_id)
        .bind(role)
        .bind(&msg.name)
        .bind(&msg.content)
        .bind(&tool_calls)
        .bind(&msg.tool_call_id)
        .bind(is_summary)
        .bind(now)
        .bind(&embedding)
        .fetch_one(&self.pool)
        .await?;

        Ok(row_id)
    }

    /// Update the embedding for an existing row by id.
    pub async fn set_embedding(&self, row_id: i64, embedding: Vector) -> anyhow::Result<()> {
        sqlx::query("UPDATE messages SET embedding = $1 WHERE id = $2")
            .bind(&embedding)
            .bind(row_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // ── Restore ───────────────────────────────────────────────────────────────

    /// Load the messages needed to reconstruct the in-memory agent history for
    /// `conversation_id`:
    ///
    /// 1. Find the most recent summary row.
    /// 2. Return [that summary row] + [all rows after it].
    ///
    /// If there is no summary, return all rows.
    pub async fn restore(&self, conversation_id: Uuid) -> anyhow::Result<Vec<Message>> {
        let since_id: i64 = sqlx::query_scalar(
            r#"
            SELECT COALESCE(
                (SELECT id FROM messages
                 WHERE conversation_id = $1 AND is_summary = true
                 ORDER BY id DESC LIMIT 1),
                0
            )
            "#,
        )
        .bind(conversation_id)
        .fetch_one(&self.pool)
        .await?;

        let rows: Vec<MessageRow> = sqlx::query_as(
            r#"
            SELECT id, conversation_id, role, name, content,
                   spell_casts, spell_cast_id, is_summary, created_at
            FROM messages
            WHERE conversation_id = $1 AND id >= $2
            ORDER BY id ASC
            "#,
        )
        .bind(conversation_id)
        .bind(since_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(row_to_message).collect())
    }

    // ── FTS search ────────────────────────────────────────────────────────────

    /// Full-text search over `content` using PostgreSQL tsvector.
    pub async fn fts_search(
        &self,
        conversation_id: Uuid,
        query: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<MessageRow>> {
        let rows: Vec<MessageRow> = sqlx::query_as(
            r#"
            SELECT id, conversation_id, role, name, content,
                   spell_casts, spell_cast_id, is_summary, created_at
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

    // ── Semantic search ───────────────────────────────────────────────────────

    /// Vector similarity search using pgvector cosine distance.
    /// Returns top-`limit` rows ordered by similarity descending.
    pub async fn semantic_search(
        &self,
        conversation_id: Uuid,
        query_vec: Vector,
        limit: usize,
    ) -> anyhow::Result<Vec<(MessageRow, f32)>> {
        let rows: Vec<(MessageRow, f32)> = sqlx::query_as(
            r#"
            SELECT id, conversation_id, role, name, content,
                   spell_casts, spell_cast_id, is_summary, created_at,
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
                    is_summary: r.is_summary,
                    created_at: r.created_at,
                },
                r.similarity,
            )
        })
        .collect();

        Ok(rows)
    }

    // ── History list (for WebUI) ──────────────────────────────────────────────

    /// Return all messages for a conversation in chronological order,
    /// for display in the WebUI (no summary filtering).
    pub async fn list_messages(&self, conversation_id: Uuid) -> anyhow::Result<Vec<MessageRow>> {
        let rows: Vec<MessageRow> = sqlx::query_as(
            r#"
            SELECT id, conversation_id, role, name, content,
                   spell_casts, spell_cast_id, is_summary, created_at
            FROM messages
            WHERE conversation_id = $1
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

/// Used only for the semantic_search query which needs an extra `similarity` column.
#[derive(sqlx::FromRow)]
struct SemanticRow {
    id: i64,
    conversation_id: Uuid,
    role: String,
    name: Option<String>,
    content: Option<String>,
    spell_casts: Option<String>,
    spell_cast_id: Option<String>,
    is_summary: bool,
    created_at: i64,
    similarity: f32,
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

pub fn role_to_str(role: &Role) -> &'static str {
    match role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
    }
}

pub fn str_to_role(s: &str) -> Role {
    match s {
        "system" => Role::System,
        "assistant" => Role::Assistant,
        "tool" => Role::Tool,
        _ => Role::User,
    }
}

pub fn row_to_message(row: MessageRow) -> Message {
    use ds_api::raw::request::message::ToolCall;

    let tool_calls: Option<Vec<ToolCall>> = row
        .spell_casts
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok());

    Message {
        role: str_to_role(&row.role),
        content: row.content,
        name: row.name,
        tool_call_id: row.spell_cast_id,
        tool_calls,
        reasoning_content: None,
        prefix: None,
    }
}

/// Encode a `Vec<f32>` to a pgvector `Vector`.
pub fn to_vector(v: Vec<f32>) -> Vector {
    Vector::from(v)
}
