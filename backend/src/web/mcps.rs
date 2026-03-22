use axum::{
    Json,
    extract::{Path, State},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use agentix::{McpTool, ToolCommand};
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

    // For stdio tools, we need to construct them per-session to use the correct sandbox.
    // For http tools, we can construct once.
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

    // Collect per-session inject senders, per-session mcp vectors, and conversation_ids for this user
    let sessions: Vec<_> = {
        let map = state.chats.lock().unwrap();
        map.iter()
            .filter(|(_, e)| e.user_id == auth.user_id)
            .map(|(&cid, e)| (cid, e.tool_inject_tx.clone(), e.user_mcp_tools.clone()))
            .collect()
    };

    for (cid, tx, mcp_vec) in sessions {
        let tool = if let Some(ref t) = http_tool {
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
            let _ = tx.send(ToolCommand::Add(Box::new(tool.clone())));
            let name = req.name.clone();
            if let Ok(mut guard) = mcp_vec.try_lock() {
                guard.retain(|(n, _)| n != &name);
                guard.push((name, tool));
            } else {
                let mcp_vec_clone = mcp_vec.clone();
                let name_clone = name.clone();
                tokio::spawn(async move {
                    let mut g = mcp_vec_clone.lock().await;
                    g.retain(|(n, _)| n != &name_clone);
                    g.push((name_clone, tool));
                });
            }
        }
    }

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

    // Per-session injection logic similar to create_mcp
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
            .map(|(&cid, e)| (cid, e.tool_inject_tx.clone(), e.user_mcp_tools.clone()))
            .collect()
    };

    for (cid, tx, mcp_vec) in sessions {
        let tool = if let Some(ref t) = http_tool {
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
            let _ = tx.send(ToolCommand::Add(Box::new(tool.clone())));
            let name = req.name.clone();
            if let Ok(mut guard) = mcp_vec.try_lock() {
                guard.retain(|(n, _)| n != &name);
                guard.push((name, tool));
            } else {
                let mcp_vec_clone = mcp_vec.clone();
                let name_clone = name.clone();
                tokio::spawn(async move {
                    let mut g = mcp_vec_clone.lock().await;
                    g.retain(|(n, _)| n != &name_clone);
                    g.push((name_clone, tool));
                });
            }
        }
    }

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

    let sessions: Vec<_> = {
        let map = state.chats.lock().unwrap();
        map.iter()
            .filter(|(_, e)| e.user_id == auth.user_id)
            .map(|(&cid, e)| (cid, e.tool_inject_tx.clone(), e.user_mcp_tools.clone()))
            .collect()
    };

    // To obtain tool names for removal, we only need to start the tool ONCE using any available session's sandbox.
    let mut tool_names: Vec<String> = Vec::new();
    if let Some(&(cid, _, _)) = sessions.first() {
        tool_names = match row.mcp_type.as_str() {
            "http" => {
                let url = row.config.get("url").and_then(|v| v.as_str()).unwrap_or_default().to_string();
                match timeout(Duration::from_secs(15), McpTool::http(&url)).await {
                    Ok(Ok(t)) => t.raw_tools().iter().map(|r| r.function.name.clone()).collect(),
                    _ => vec![],
                }
            }
            "stdio" => {
                let command = row.config.get("command").and_then(|v| v.as_str()).unwrap_or_default().to_string();
                let args: Vec<String> = row.config.get("args").and_then(|v| v.as_array()).map(|a| {
                    a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()
                }).unwrap_or_default();
                let args_ref: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
                let (cmd, args_wrapped_vec) = state.sandbox.wrap_mcp_command(auth.user_id, cid, &command, &args_ref);
                let args_wrapped: Vec<&str> = args_wrapped_vec.iter().map(|s| s.as_str()).collect();

                match timeout(Duration::from_secs(300), McpTool::stdio(&cmd, &args_wrapped)).await {
                    Ok(Ok(t)) => t.raw_tools().iter().map(|r| r.function.name.clone()).collect(),
                    _ => vec![],
                }
            }
            _ => vec![],
        };
    }

    let result = sqlx::query("DELETE FROM user_mcps WHERE id = $1 AND user_id = $2")
        .bind(id)
        .bind(auth.user_id)
        .execute(&state.pool)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::not_found("MCP 不存在"));
    }

    if !tool_names.is_empty() {
        for (_, tx, mcp_vec) in sessions {
            let _ = tx.send(ToolCommand::Remove(tool_names.clone()));
            if let Ok(mut guard) = mcp_vec.try_lock() {
                guard.retain(|(n, _)| !tool_names.iter().any(|tn| n == tn));
            } else {
                let mcp_vec_clone = mcp_vec.clone();
                let names = tool_names.clone();
                tokio::spawn(async move {
                    let mut g = mcp_vec_clone.lock().await;
                    g.retain(|(n, _)| !names.iter().any(|tn| n == tn));
                });
            }
        }
    }

    Ok(Json(json!({ "ok": true })))
}
