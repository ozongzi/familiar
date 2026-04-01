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
    pub model_id: Option<Uuid>,
    pub created_at: String,
}

#[derive(Deserialize)]
pub struct CreateConversationRequest {
    pub name: Option<String>,
    pub model_id: Option<Uuid>,
}

pub async fn list_conversations(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<Vec<ConversationResponse>>> {
    let rows = sqlx::query(
        r#"
        SELECT id, name, model_id, created_at
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
            let model_id: Option<Uuid> = r
                .try_get("model_id")
                .map_err(|_| AppError::internal("db error"))?;
            let created_at: chrono::DateTime<chrono::Utc> = r
                .try_get("created_at")
                .map_err(|_| AppError::internal("db error"))?;
            Ok(ConversationResponse {
                id,
                name,
                model_id,
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
        INSERT INTO conversations (user_id, name, model_id)
        VALUES ($1, $2, $3)
        RETURNING id, name, model_id, created_at
        "#,
    )
    .bind(auth.user_id)
    .bind(&name)
    .bind(req.model_id)
    .fetch_one(&state.pool)
    .await?;

    let id: Uuid = row
        .try_get("id")
        .map_err(|_| AppError::internal("db error"))?;
    let name: String = row
        .try_get("name")
        .map_err(|_| AppError::internal("db error"))?;
    let model_id: Option<Uuid> = row
        .try_get("model_id")
        .map_err(|_| AppError::internal("db error"))?;
    let created_at: chrono::DateTime<chrono::Utc> = row
        .try_get("created_at")
        .map_err(|_| AppError::internal("db error"))?;

    Ok(Json(ConversationResponse {
        id,
        name,
        model_id,
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

#[derive(Deserialize)]
pub struct AutoTitleRequest {
    pub prompt: String,
}

#[derive(Serialize)]
pub struct AutoTitleResponse {
    pub title: String,
}

pub async fn auto_title(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<AutoTitleRequest>,
) -> AppResult<Json<AutoTitleResponse>> {
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

    let global_cfg = state.get_global_config().await?;
    let cm = &global_cfg.cheap_model;

    let prompt = format!(
        "根据用户发送的第一条消息 {}，生成一个简短的对话标题（5到10个字）。只返回标题文字本身，不加引号、标点或任何解释。",
        &req.prompt
    );

    let http = reqwest::Client::new();
    let resp = cm
        .to_request()
        .max_tokens(64)
        .user(prompt)
        .complete(&http)
        .await
        .map_err(|e: agentix::ApiError| AppError::internal(&e.to_string()))?;

    let title = resp.content.unwrap_or_default().trim().to_string();

    Ok(Json(AutoTitleResponse { title }))
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
        RETURNING id, name, model_id, created_at
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
    let model_id: Option<Uuid> = row
        .try_get("model_id")
        .map_err(|_| AppError::internal("db error"))?;
    let created_at: chrono::DateTime<chrono::Utc> = row
        .try_get("created_at")
        .map_err(|_| AppError::internal("db error"))?;

    Ok(Json(ConversationResponse {
        id,
        name,
        model_id,
        created_at: created_at.to_rfc3339(),
    }))
}
