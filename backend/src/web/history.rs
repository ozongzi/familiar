use axum::{
    Json,
    extract::{Path, Query, State},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::web::AppState;
use crate::web::auth::AuthUser;

#[derive(Serialize)]
pub struct MessageResponse {
    pub id: i64,
    pub role: String,
    pub name: Option<String>,
    pub content: Option<String>,
    pub tool_calls: Option<String>,
    pub tool_call_id: Option<String>,
    pub created_at: i64,
    pub streaming: bool,
    pub reasoning: Option<String>,
    /// Parent message id — lets the client reconstruct the tree.
    pub parent_id: Option<i64>,
    /// Ids of all messages sharing this one's `parent_id` (including self),
    /// in id order. When `siblings.len() > 1`, the UI renders branch
    /// switcher arrows on this message.
    pub siblings: Vec<i64>,
    /// Non-null on compaction anchor messages: points back to the oldest
    /// message that should still be visible to the LLM. The frontend uses
    /// this to (a) detect pending/failed compactions and (b) optionally
    /// fade messages older than the cutoff.
    pub summary_start_id: Option<i64>,
}

#[derive(Deserialize)]
pub struct SearchQuery {
    pub q: String,
    pub limit: Option<usize>,
}

pub async fn search_messages(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(params): Query<SearchQuery>,
) -> AppResult<Json<serde_json::Value>> {
    let q = params.q.trim().to_string();
    if q.is_empty() {
        return Ok(Json(serde_json::json!({ "results": [] })));
    }
    let limit = params.limit.unwrap_or(20).min(50);

    let rows = sqlx::query(
        r#"
        SELECT m.id, m.conversation_id, m.role, m.content, m.created_at,
               c.name AS conv_name
        FROM messages m
        JOIN conversations c ON c.id = m.conversation_id
        WHERE c.user_id = $1
          AND m.content_tsv @@ plainto_tsquery('simple', $2)
          AND m.streaming = false
          AND m.content IS NOT NULL AND m.content != ''
        ORDER BY m.id DESC
        LIMIT $3
        "#,
    )
    .bind(auth.user_id)
    .bind(&q)
    .bind(limit as i64)
    .fetch_all(&state.pool)
    .await?;

    use sqlx::Row;
    let results: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|r| {
            let id: i64 = r.try_get("id").unwrap_or(0);
            let conv_id: uuid::Uuid = r.try_get("conversation_id").unwrap_or_default();
            let role: String = r.try_get("role").unwrap_or_default();
            let content: Option<String> = r.try_get("content").unwrap_or(None);
            let created_at: i64 = r.try_get("created_at").unwrap_or(0);
            let conv_name: String = r.try_get("conv_name").unwrap_or_default();
            serde_json::json!({
                "id": id,
                "conversation_id": conv_id,
                "conversation_name": conv_name,
                "role": role,
                "content": content,
                "created_at": created_at,
            })
        })
        .collect();

    Ok(Json(serde_json::json!({ "results": results })))
}

pub async fn list_messages(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Vec<MessageResponse>>> {
    // Verify ownership.
    let owned: bool = sqlx::query_scalar::<_, Option<bool>>(
        "SELECT EXISTS(SELECT 1 FROM conversations WHERE id = $1 AND user_id = $2)",
    )
    .bind(id)
    .bind(auth.user_id)
    .fetch_one(&state.pool)
    .await?
    .unwrap_or(false);

    if !owned {
        return Err(AppError::not_found("对话不存在"));
    }

    let rows = state.db.list_active_with_siblings(id).await?;

    Ok(Json(
        rows.into_iter()
            .map(|(r, siblings)| MessageResponse {
                id: r.id,
                role: r.role,
                name: r.name,
                content: r.content,
                tool_calls: r.spell_casts,
                tool_call_id: r.spell_cast_id,
                created_at: r.created_at,
                streaming: r.streaming,
                reasoning: r.reasoning,
                parent_id: r.parent_id,
                siblings,
                summary_start_id: r.summary_start_id,
            })
            .collect(),
    ))
}
