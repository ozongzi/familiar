use axum::{
    Json,
    extract::{Path, State},
};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use sqlx::types::chrono;
use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::web::AppState;
use crate::web::auth::AuthUser;

#[derive(Serialize)]
pub struct ConversationResponse {
    pub id: Uuid,
    pub name: String,
    pub created_at: String,
}

#[derive(Deserialize)]
pub struct CreateConversationRequest {
    pub name: Option<String>,
}

pub async fn list_conversations(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<Vec<ConversationResponse>>> {
    let rows = sqlx::query(
        r#"
        SELECT id, name, created_at
        FROM conversations
        WHERE user_id = $1
        ORDER BY created_at DESC
        "#,
    )
    .bind(auth.user_id)
    .fetch_all(&state.pool)
    .await?;

    let result = rows
        .into_iter()
        .map(|r| {
            let id: Uuid = r
                .try_get("id")
                .map_err(|_| AppError::internal("db error"))?;
            let name: String = r
                .try_get("name")
                .map_err(|_| AppError::internal("db error"))?;
            let created_at: chrono::DateTime<chrono::Utc> = r
                .try_get("created_at")
                .map_err(|_| AppError::internal("db error"))?;
            Ok(ConversationResponse {
                id,
                name,
                created_at: created_at.to_rfc3339(),
            })
        })
        .collect::<AppResult<Vec<_>>>()?;

    Ok(Json(result))
}

pub async fn create_conversation(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<CreateConversationRequest>,
) -> AppResult<Json<ConversationResponse>> {
    let name = req.name.unwrap_or_else(|| "新对话".to_string());

    let row = sqlx::query(
        r#"
        INSERT INTO conversations (user_id, name)
        VALUES ($1, $2)
        RETURNING id, name, created_at
        "#,
    )
    .bind(auth.user_id)
    .bind(&name)
    .fetch_one(&state.pool)
    .await?;

    let id: Uuid = row
        .try_get("id")
        .map_err(|_| AppError::internal("db error"))?;
    let name: String = row
        .try_get("name")
        .map_err(|_| AppError::internal("db error"))?;
    let created_at: chrono::DateTime<chrono::Utc> = row
        .try_get("created_at")
        .map_err(|_| AppError::internal("db error"))?;

    Ok(Json(ConversationResponse {
        id,
        name,
        created_at: created_at.to_rfc3339(),
    }))
}

pub async fn delete_conversation(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<serde_json::Value>> {
    let result = sqlx::query("DELETE FROM conversations WHERE id = $1 AND user_id = $2")
        .bind(id)
        .bind(auth.user_id)
        .execute(&state.pool)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::not_found("对话不存在"));
    }

    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn rename_conversation(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<CreateConversationRequest>,
) -> AppResult<Json<ConversationResponse>> {
    let name = req
        .name
        .ok_or_else(|| AppError::bad_request("name 不能为空"))?;

    let row = sqlx::query(
        r#"
        UPDATE conversations
        SET name = $1
        WHERE id = $2 AND user_id = $3
        RETURNING id, name, created_at
        "#,
    )
    .bind(&name)
    .bind(id)
    .bind(auth.user_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::not_found("对话不存在"))?;

    let id: Uuid = row
        .try_get("id")
        .map_err(|_| AppError::internal("db error"))?;
    let name: String = row
        .try_get("name")
        .map_err(|_| AppError::internal("db error"))?;
    let created_at: chrono::DateTime<chrono::Utc> = row
        .try_get("created_at")
        .map_err(|_| AppError::internal("db error"))?;

    Ok(Json(ConversationResponse {
        id,
        name,
        created_at: created_at.to_rfc3339(),
    }))
}
