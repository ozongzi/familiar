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
    pub folder_id: Option<Uuid>,
}

#[derive(Serialize)]
pub struct FolderResponse {
    pub id: Uuid,
    pub name: String,
    pub parent_id: Option<Uuid>,
    pub position: i32,
    pub created_at: String,
    pub conversation_count: i64,
}

#[derive(Deserialize)]
pub struct CreateFolderRequest {
    pub name: String,
    pub parent_id: Option<Uuid>,
}

#[derive(Deserialize)]
pub struct UpdateFolderRequest {
    pub name: Option<String>,
    pub parent_id: Option<Option<Uuid>>,
    pub position: Option<i32>,
}

#[derive(Deserialize)]
pub struct MoveConversationRequest {
    pub folder_id: Option<Uuid>,
}

#[derive(Deserialize)]
pub struct CreateConversationRequest {
    pub name: Option<String>,
    pub model_id: Option<Uuid>,
    pub folder_id: Option<Uuid>,
}

pub async fn list_conversations(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<Vec<ConversationResponse>>> {
    let rows = sqlx::query(
        r#"
        SELECT id, name, model_id, created_at, folder_id
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
            let folder_id: Option<Uuid> = r
                .try_get("folder_id")
                .map_err(|_| AppError::internal("db error"))?;
            Ok(ConversationResponse {
                id,
                name,
                model_id,
                created_at: created_at.to_rfc3339(),
                folder_id,
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

    if let Some(model_id) = req.model_id {
        let allowed: Option<bool> = sqlx::query_scalar(
            "SELECT EXISTS(
                    SELECT 1
                    FROM models m
                    WHERE m.id = $1
                      AND (
                        (m.scope = 'user' AND m.user_id = $2)
                        OR (
                            m.scope = 'global'
                            AND COALESCE(
                                (
                                    SELECT allowed
                                    FROM user_model_permissions ump
                                    WHERE ump.user_id = $2 AND ump.model_id = m.id
                                ),
                                m.initial_available
                            )
                        )
                      )
                )",
        )
        .bind(model_id)
        .bind(auth.user_id)
        .fetch_optional(&state.pool)
        .await?;
        if allowed != Some(true) {
            return Err(AppError::forbidden("该模型未对当前用户开放"));
        }
    }

    // Snapshot the user's effective default model so the conversation always
    // has a permanent model_id. Without this, conversations created without an
    // explicit model_id used to fall back to "current default at generation
    // time" — which made cost attribution and audit useless once the default
    // changed. NULL is still possible when the user has no default available;
    // that's preserved for compatibility with the cheap-model fallback path.
    let model_id: Option<Uuid> = match req.model_id {
        Some(id) => Some(id),
        None => {
            sqlx::query_scalar(
                "SELECT id FROM models
             WHERE scope = 'global'
               AND is_default = true
               AND COALESCE(
                    (
                        SELECT allowed
                        FROM user_model_permissions ump
                        WHERE ump.user_id = $1 AND ump.model_id = models.id
                    ),
                    initial_available
               )
             LIMIT 1",
            )
            .bind(auth.user_id)
            .fetch_optional(&state.pool)
            .await?
        }
    };

    let row = sqlx::query(
        r#"
        INSERT INTO conversations (user_id, name, model_id, folder_id)
        VALUES ($1, $2, $3, $4)
        RETURNING id, name, model_id, created_at, folder_id
        "#,
    )
    .bind(auth.user_id)
    .bind(&name)
    .bind(model_id)
    .bind(req.folder_id)
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
    let folder_id: Option<Uuid> = row
        .try_get("folder_id")
        .map_err(|_| AppError::internal("db error"))?;

    persist_initial_memory_snapshot(&state, auth.user_id, id).await;

    Ok(Json(ConversationResponse {
        id,
        name,
        model_id,
        created_at: created_at.to_rfc3339(),
        folder_id,
    }))
}

async fn persist_initial_memory_snapshot(state: &AppState, user_id: Uuid, conversation_id: Uuid) {
    let Some(memory) =
        crate::spells::load_memories_for_prompt(&state.pool, user_id, conversation_id).await
    else {
        return;
    };

    let content = format!(
        "（以下为持久背景记忆，自然地当作已知信息使用，不要提及此机制）\n\n{}",
        memory.trim()
    );
    let now = chrono::Utc::now().timestamp();

    let result = async {
        let mut tx = state.pool.begin().await?;
        let msg_id: i64 = sqlx::query_scalar(
            r#"
            INSERT INTO messages
                (conversation_id, role, name, content, created_at, streaming)
            VALUES ($1, 'user', NULL, $2, $3, FALSE)
            RETURNING id
            "#,
        )
        .bind(conversation_id)
        .bind(&content)
        .bind(now)
        .fetch_one(&mut *tx)
        .await?;

        sqlx::query("UPDATE conversations SET active_message_id = $1 WHERE id = $2")
            .bind(msg_id)
            .bind(conversation_id)
            .execute(&mut *tx)
            .await?;

        tx.commit().await
    }
    .await;

    if let Err(err) = result {
        tracing::warn!(
            user_id = %user_id,
            conversation_id = %conversation_id,
            error = %err,
            "failed to persist initial memory snapshot"
        );
    }
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

    if let Err(err) = state
        .sandbox
        .remove_conversation_resources(auth.user_id, id)
    {
        tracing::error!(
            user_id = %auth.user_id,
            conversation_id = %id,
            error = %err,
            "failed to remove conversation sandbox resources"
        );
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
        RETURNING id, name, model_id, created_at, folder_id
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
    let folder_id: Option<Uuid> = row
        .try_get("folder_id")
        .map_err(|_| AppError::internal("db error"))?;

    Ok(Json(ConversationResponse {
        id,
        name,
        model_id,
        created_at: created_at.to_rfc3339(),
        folder_id,
    }))
}

// ── Folder handlers ─────────────────────────────────────────────────────────

pub async fn list_folders(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<Vec<FolderResponse>>> {
    let rows = sqlx::query(
        r#"
        SELECT f.id, f.name, f.parent_id, f.position, f.created_at,
               COUNT(c.id) AS conversation_count
        FROM folders f
        LEFT JOIN conversations c ON c.folder_id = f.id
        WHERE f.user_id = $1
        GROUP BY f.id, f.name, f.parent_id, f.position, f.created_at
        ORDER BY f.position ASC, f.created_at ASC
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
            let parent_id: Option<Uuid> = r
                .try_get("parent_id")
                .map_err(|_| AppError::internal("db error"))?;
            let position: i32 = r
                .try_get("position")
                .map_err(|_| AppError::internal("db error"))?;
            let created_at: chrono::DateTime<chrono::Utc> = r
                .try_get("created_at")
                .map_err(|_| AppError::internal("db error"))?;
            let conversation_count: i64 = r
                .try_get("conversation_count")
                .map_err(|_| AppError::internal("db error"))?;
            Ok(FolderResponse {
                id,
                name,
                parent_id,
                position,
                created_at: created_at.to_rfc3339(),
                conversation_count,
            })
        })
        .collect::<AppResult<Vec<_>>>()?;

    Ok(Json(result))
}

pub async fn create_folder(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<CreateFolderRequest>,
) -> AppResult<Json<FolderResponse>> {
    // Validate parent_id belongs to user if provided
    if let Some(parent_id) = req.parent_id {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM folders WHERE id = $1 AND user_id = $2)",
        )
        .bind(parent_id)
        .bind(auth.user_id)
        .fetch_one(&state.pool)
        .await?;
        if !exists {
            return Err(AppError::not_found("父文件夹不存在"));
        }
    }

    let row = sqlx::query(
        r#"
        INSERT INTO folders (user_id, name, parent_id)
        VALUES ($1, $2, $3)
        RETURNING id, name, parent_id, position, created_at
        "#,
    )
    .bind(auth.user_id)
    .bind(&req.name)
    .bind(req.parent_id)
    .fetch_one(&state.pool)
    .await?;

    let id: Uuid = row
        .try_get("id")
        .map_err(|_| AppError::internal("db error"))?;
    let name: String = row
        .try_get("name")
        .map_err(|_| AppError::internal("db error"))?;
    let parent_id: Option<Uuid> = row
        .try_get("parent_id")
        .map_err(|_| AppError::internal("db error"))?;
    let position: i32 = row
        .try_get("position")
        .map_err(|_| AppError::internal("db error"))?;
    let created_at: chrono::DateTime<chrono::Utc> = row
        .try_get("created_at")
        .map_err(|_| AppError::internal("db error"))?;

    Ok(Json(FolderResponse {
        id,
        name,
        parent_id,
        position,
        created_at: created_at.to_rfc3339(),
        conversation_count: 0,
    }))
}

pub async fn update_folder(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateFolderRequest>,
) -> AppResult<Json<FolderResponse>> {
    // First verify ownership
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM folders WHERE id = $1 AND user_id = $2)")
            .bind(id)
            .bind(auth.user_id)
            .fetch_one(&state.pool)
            .await?;
    if !exists {
        return Err(AppError::not_found("文件夹不存在"));
    }

    if let Some(name) = &req.name {
        sqlx::query("UPDATE folders SET name = $1 WHERE id = $2")
            .bind(name)
            .bind(id)
            .execute(&state.pool)
            .await?;
    }

    if let Some(parent) = req.parent_id {
        // Prevent circular reference: check that the new parent is not this folder
        if parent == Some(id) {
            return Err(AppError::bad_request("不能将文件夹移动到自身下"));
        }
        sqlx::query("UPDATE folders SET parent_id = $1 WHERE id = $2")
            .bind(parent)
            .bind(id)
            .execute(&state.pool)
            .await?;
    }

    if let Some(pos) = req.position {
        sqlx::query("UPDATE folders SET position = $1 WHERE id = $2")
            .bind(pos)
            .bind(id)
            .execute(&state.pool)
            .await?;
    }

    // Return updated folder
    let row = sqlx::query(
        r#"
        SELECT f.id, f.name, f.parent_id, f.position, f.created_at,
               COUNT(c.id) AS conversation_count
        FROM folders f
        LEFT JOIN conversations c ON c.folder_id = f.id
        WHERE f.id = $1
        GROUP BY f.id, f.name, f.parent_id, f.position, f.created_at
        "#,
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await?;

    Ok(Json(FolderResponse {
        id: row
            .try_get("id")
            .map_err(|_| AppError::internal("db error"))?,
        name: row
            .try_get("name")
            .map_err(|_| AppError::internal("db error"))?,
        parent_id: row
            .try_get("parent_id")
            .map_err(|_| AppError::internal("db error"))?,
        position: row
            .try_get("position")
            .map_err(|_| AppError::internal("db error"))?,
        created_at: row
            .get::<chrono::DateTime<chrono::Utc>, _>("created_at")
            .to_rfc3339(),
        conversation_count: row
            .try_get("conversation_count")
            .map_err(|_| AppError::internal("db error"))?,
    }))
}

pub async fn delete_folder(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<serde_json::Value>> {
    let result = sqlx::query("DELETE FROM folders WHERE id = $1 AND user_id = $2")
        .bind(id)
        .bind(auth.user_id)
        .execute(&state.pool)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::not_found("文件夹不存在"));
    }

    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn move_conversation(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<MoveConversationRequest>,
) -> AppResult<Json<serde_json::Value>> {
    // Verify conversation ownership
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM conversations WHERE id = $1 AND user_id = $2)",
    )
    .bind(id)
    .bind(auth.user_id)
    .fetch_one(&state.pool)
    .await?;
    if !exists {
        return Err(AppError::not_found("对话不存在"));
    }

    // If moving to a folder, verify folder ownership
    if let Some(folder_id) = req.folder_id {
        let folder_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM folders WHERE id = $1 AND user_id = $2)",
        )
        .bind(folder_id)
        .bind(auth.user_id)
        .fetch_one(&state.pool)
        .await?;
        if !folder_exists {
            return Err(AppError::not_found("目标文件夹不存在"));
        }
    }

    sqlx::query("UPDATE conversations SET folder_id = $1 WHERE id = $2")
        .bind(req.folder_id)
        .bind(id)
        .execute(&state.pool)
        .await?;

    Ok(Json(serde_json::json!({ "ok": true })))
}
