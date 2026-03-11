use axum::{
    Json,
    extract::{Path, State},
};
use serde::Serialize;
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
    pub is_summary: bool,
    pub created_at: i64,
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

    let rows = state.db.list_messages(id).await?;

    Ok(Json(
        rows.into_iter()
            .map(|r| MessageResponse {
                id: r.id,
                role: r.role,
                name: r.name,
                content: r.content,
                tool_calls: r.spell_casts,
                tool_call_id: r.spell_cast_id,
                is_summary: r.is_summary,
                created_at: r.created_at,
            })
            .collect(),
    ))
}
