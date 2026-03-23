use axum::{
    Json,
    extract::{Path, State},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use agentix::McpTool;
use std::time::Duration;
use tokio::time::timeout;

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
        "http" if req.config.get("url").and_then(|v| v.as_str()).is_none() => {
            return Err(AppError::bad_request("http 类型需要 config.url"));
        }
        "stdio" if req.config.get("command").and_then(|v| v.as_str()).is_none() => {
            return Err(AppError::bad_request("stdio 类型需要 config.command"));
        }
        _ => {}
    }

    let row = sqlx::query_as::<_, McpRow>(
        r#"INSERT INTO user_mcps (user_id, name, "type", config)
           VALUES ($1, $2, $3, $4)
           RETURNING id, name, "type" AS mcp_type, config, created_at"#,
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

    // Eagerly connect the new tool and push it into all active sessions for this user.
    // For http tools we connect once; for stdio we must connect per-session with the
    // correct sandbox wrapping.
    let http_tool: Option<McpTool> = if req.r#type == "http" {
        if let Some(url) = req.config.get("url").and_then(|v| v.as_str()) {
            match timeout(Duration::from_secs(15), McpTool::http(url)).await {
                Ok(Ok(t)) => Some(t),
                _ => None,
            }
        } else {
            None
        }
    } else {
        None
    };

    let sessions: Vec<_> = {
        let map = state.chats.lock().unwrap();
        map.iter()
            .filter(|(_, e)| e.user_id == auth.user_id)
            .map(|(&cid, e)| (cid, e.user_mcp_tools.clone(), std::sync::Arc::clone(&e.agent)))
            .collect()
    };

    for (cid, mcp_vec, agent_arc) in sessions {
        let tool: Option<McpTool> = if let Some(ref t) = http_tool {
            Some(t.clone())
        } else if req.r#type == "stdio" {
            let command = req.config.get("command").and_then(|v| v.as_str()).unwrap_or_default();
            let args: Vec<String> = req.config.get("args").and_then(|v| v.as_array()).map(|a| {
                a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()
            }).unwrap_or_default();
            let args_ref: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            let (cmd, args_wrapped_vec) = state.sandbox.wrap_mcp_command(auth.user_id, cid, command, &args_ref);
            let args_wrapped: Vec<&str> = args_wrapped_vec.iter().map(|s| s.as_str()).collect();

            match timeout(Duration::from_secs(300), McpTool::stdio(&cmd, &args_wrapped)).await {
                Ok(Ok(t)) => Some(t),
                _ => None,
            }
        } else {
            None
        };

        if let Some(tool) = tool {
            let name = req.name.clone();
            {
                let mut guard = mcp_vec.lock().await;
                guard.retain(|(n, _)| n != &name);
                guard.push((name, tool.clone()));
            }
            agent_arc.lock().await.add_tool(tool).await;
        }
    }

    let _ = crate::audit::log_audit(
        &state.pool,
        Some(auth.user_id),
        None,
        "mcp.create",
        Some(serde_json::json!({ "name": req.name, "type": req.r#type })),
        None,
    ).await;

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
           RETURNING id, name, "type" AS mcp_type, config, created_at"#,
    )
    .bind(&req.name)
    .bind(&req.r#type)
    .bind(&req.config)
    .bind(id)
    .bind(auth.user_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::not_found("MCP 不存在"))?;

    let http_tool: Option<McpTool> = if req.r#type == "http" {
        if let Some(url) = req.config.get("url").and_then(|v| v.as_str()) {
            match timeout(Duration::from_secs(15), McpTool::http(url)).await {
                Ok(Ok(t)) => Some(t),
                _ => None,
            }
        } else {
            None
        }
    } else {
        None
    };

    let sessions: Vec<_> = {
        let map = state.chats.lock().unwrap();
        map.iter()
            .filter(|(_, e)| e.user_id == auth.user_id)
            .map(|(&cid, e)| (cid, e.user_mcp_tools.clone(), std::sync::Arc::clone(&e.agent)))
            .collect()
    };

    for (cid, mcp_vec, agent_arc) in sessions {
        let tool: Option<McpTool> = if let Some(ref t) = http_tool {
            Some(t.clone())
        } else if req.r#type == "stdio" {
            let command = req.config.get("command").and_then(|v| v.as_str()).unwrap_or_default();
            let args: Vec<String> = req.config.get("args").and_then(|v| v.as_array()).map(|a| {
                a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()
            }).unwrap_or_default();
            let args_ref: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            let (cmd, args_wrapped_vec) = state.sandbox.wrap_mcp_command(auth.user_id, cid, command, &args_ref);
            let args_wrapped: Vec<&str> = args_wrapped_vec.iter().map(|s| s.as_str()).collect();

            match timeout(Duration::from_secs(300), McpTool::stdio(&cmd, &args_wrapped)).await {
                Ok(Ok(t)) => Some(t),
                _ => None,
            }
        } else {
            None
        };

        if let Some(tool) = tool {
            let name = req.name.clone();
            {
                let mut guard = mcp_vec.lock().await;
                guard.retain(|(n, _)| n != &name);
                guard.push((name, tool.clone()));
            }
            agent_arc.lock().await.add_tool(tool).await;
        }
    }

    let _ = crate::audit::log_audit(
        &state.pool,
        Some(auth.user_id),
        None,
        "mcp.update",
        Some(serde_json::json!({ "id": id, "name": req.name, "type": req.r#type })),
        None,
    ).await;

    Ok(Json(McpResponse::from(row)))
}

pub async fn delete_mcp(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Value>> {
    let existing = sqlx::query_as::<_, McpRow>(
        r#"SELECT id, name, "type" AS mcp_type, config, created_at FROM user_mcps WHERE id = $1 AND user_id = $2"#
    )
    .bind(id)
    .bind(auth.user_id)
    .fetch_optional(&state.pool)
    .await?;

    let row = existing.ok_or_else(|| AppError::not_found("MCP 不存在"))?;

    let result = sqlx::query("DELETE FROM user_mcps WHERE id = $1 AND user_id = $2")
        .bind(id)
        .bind(auth.user_id)
        .execute(&state.pool)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::not_found("MCP 不存在"));
    }

    // Remove from all active sessions for this user and hot-swap the agent's tool bundle
    let sessions: Vec<_> = {
        let map = state.chats.lock().unwrap();
        map.iter()
            .filter(|(_, e)| e.user_id == auth.user_id)
            .map(|(_, e)| (e.user_mcp_tools.clone(), std::sync::Arc::clone(&e.agent)))
            .collect()
    };

    let name = row.name.clone();
    for (mcp_vec, agent_arc) in sessions {
        // Remove from the session's MCP list
        let tool_fn_names: Vec<String> = {
            let mut guard = mcp_vec.lock().await;
            let names = guard
                .iter()
                .filter(|(n, _)| n == &name)
                .flat_map(|(_, t)| t.raw_tools().into_iter().map(|r| r.function.name.clone()))
                .collect();
            guard.retain(|(n, _)| n != &name);
            names
        };
        // Hot-remove each tool function from the running agent by name
        let mut agent = agent_arc.lock().await;
        for fn_name in &tool_fn_names {
            agent.delete_tool(fn_name).await;
        }
    }

    let _ = crate::audit::log_audit(
        &state.pool,
        Some(auth.user_id),
        None,
        "mcp.delete",
        Some(serde_json::json!({ "id": id, "name": name })),
        None,
    ).await;

    Ok(Json(json!({ "ok": true })))
}
