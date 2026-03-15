use axum::{
    Json,
    extract::{Path, State},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::web::AppState;
use crate::web::auth::AuthUser;

#[derive(Serialize)]
pub struct McpResponse {
    pub id: Uuid,
    pub name: String,
    pub r#type: String,
    pub config: Value,
    pub created_at: String,
}

#[derive(Deserialize)]
pub struct CreateMcpRequest {
    pub name: String,
    pub r#type: String,
    pub config: Value,
}

#[derive(sqlx::FromRow)]
struct McpRow {
    id: Uuid,
    name: String,
    mcp_type: String,
    config: Value,
    created_at: sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>,
}

impl From<McpRow> for McpResponse {
    fn from(r: McpRow) -> Self {
        McpResponse {
            id: r.id,
            name: r.name,
            r#type: r.mcp_type,
            config: r.config,
            created_at: r.created_at.to_rfc3339(),
        }
    }
}

pub async fn list_mcps(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<Vec<McpResponse>>> {
    let rows = sqlx::query_as::<_, McpRow>(
        r#"SELECT id, name, "type" AS mcp_type, config, created_at FROM user_mcps WHERE user_id = $1 ORDER BY created_at ASC"#
    )
    .bind(auth.user_id)
    .fetch_all(&state.pool)
    .await?;

    Ok(Json(rows.into_iter().map(McpResponse::from).collect()))
}

pub async fn create_mcp(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<CreateMcpRequest>,
) -> AppResult<Json<McpResponse>> {
    if req.r#type != "http" && req.r#type != "stdio" {
        return Err(AppError::bad_request("type 必须是 http 或 stdio"));
    }
    match req.r#type.as_str() {
        "http" if req.config.get("url").and_then(|v| v.as_str()).is_none() =>
            return Err(AppError::bad_request("http 类型需要 config.url")),
        "stdio" if req.config.get("command").and_then(|v| v.as_str()).is_none() =>
            return Err(AppError::bad_request("stdio 类型需要 config.command")),
        _ => {}
    }

    let row = sqlx::query_as::<_, McpRow>(
        r#"INSERT INTO user_mcps (user_id, name, "type", config)
           VALUES ($1, $2, $3, $4)
           RETURNING id, name, "type" AS mcp_type, config, created_at"#
    )
    .bind(auth.user_id)
    .bind(&req.name)
    .bind(&req.r#type)
    .bind(&req.config)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| {
        if e.to_string().contains("unique") {
            AppError::bad_request("已存在同名 MCP")
        } else {
            AppError::internal(&e.to_string())
        }
    })?;

    Ok(Json(McpResponse::from(row)))
}

pub async fn update_mcp(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<CreateMcpRequest>,
) -> AppResult<Json<McpResponse>> {
    if req.r#type != "http" && req.r#type != "stdio" {
        return Err(AppError::bad_request("type 必须是 http 或 stdio"));
    }

    let row = sqlx::query_as::<_, McpRow>(
        r#"UPDATE user_mcps SET name = $1, "type" = $2, config = $3
           WHERE id = $4 AND user_id = $5
           RETURNING id, name, "type" AS mcp_type, config, created_at"#
    )
    .bind(&req.name)
    .bind(&req.r#type)
    .bind(&req.config)
    .bind(id)
    .bind(auth.user_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::not_found("MCP 不存在"))?;

    Ok(Json(McpResponse::from(row)))
}

pub async fn delete_mcp(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Value>> {
    let result = sqlx::query(
        "DELETE FROM user_mcps WHERE id = $1 AND user_id = $2"
    )
    .bind(id)
    .bind(auth.user_id)
    .execute(&state.pool)
    .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::not_found("MCP 不存在"));
    }

    Ok(Json(json!({ "ok": true })))
}
