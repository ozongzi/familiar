use axum::{
    Json,
    extract::{Path, State},
};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use uuid::Uuid;

use agentix::McpTool;
use crate::config::{Config, McpServerConfig};
use crate::errors::{AppError, AppResult};
use crate::web::{AppState, auth::AuthUser};
use tracing::warn;

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct AppSkill {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub content: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Deserialize)]
pub struct AppSkillRequest {
    pub name: String,
    pub description: Option<String>,
    pub content: String,
}

fn guard_admin(auth: &AuthUser) -> AppResult<()> {
    if !auth.is_admin {
        return Err(AppError::forbidden("仅管理员可访问"));
    }
    Ok(())
}

pub async fn get_admin_config(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<Config>> {
    guard_admin(&auth)?;
    let cfg = Config::load_from_db(&state.pool)
        .await
        .map_err(|e| AppError::internal(&e.to_string()))?;
    Ok(Json(cfg))
}

pub async fn update_admin_config(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(cfg): Json<Config>,
) -> AppResult<Json<serde_json::Value>> {
    guard_admin(&auth)?;
    Config::upsert(&state.pool, &cfg)
        .await
        .map_err(|e| AppError::internal(&e.to_string()))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn list_app_skills(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<Vec<AppSkill>>> {
    guard_admin(&auth)?;
    let rows = sqlx::query_as::<_, AppSkill>(
        "SELECT id, name, description, content, created_at, updated_at FROM app_skills ORDER BY name ASC",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

pub async fn create_app_skill(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<AppSkillRequest>,
) -> AppResult<Json<AppSkill>> {
    guard_admin(&auth)?;
    let row = sqlx::query_as::<_, AppSkill>(
        r#"
        INSERT INTO app_skills (name, description, content)
        VALUES ($1, $2, $3)
        RETURNING id, name, description, content, created_at, updated_at
        "#,
    )
    .bind(req.name)
    .bind(req.description)
    .bind(req.content)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| {
        if e.to_string().contains("unique") {
            AppError::bad_request("已存在同名默认 Skill")
        } else {
            AppError::internal(&e.to_string())
        }
    })?;

    Ok(Json(row))
}

pub async fn update_app_skill(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<AppSkillRequest>,
) -> AppResult<Json<AppSkill>> {
    guard_admin(&auth)?;
    let row = sqlx::query_as::<_, AppSkill>(
        r#"
        UPDATE app_skills
        SET name = $1, description = $2, content = $3, updated_at = NOW()
        WHERE id = $4
        RETURNING id, name, description, content, created_at, updated_at
        "#,
    )
    .bind(req.name)
    .bind(req.description)
    .bind(req.content)
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::not_found("默认 Skill 不存在"))?;

    Ok(Json(row))
}

pub async fn delete_app_skill(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<serde_json::Value>> {
    guard_admin(&auth)?;

    let res = sqlx::query("DELETE FROM app_skills WHERE id = $1")
        .bind(id)
        .execute(&state.pool)
        .await?;

    if res.rows_affected() == 0 {
        return Err(AppError::not_found("默认 Skill 不存在"));
    }

    Ok(Json(serde_json::json!({"ok": true})))
}

// ── User Management ──────────────────────────────────────────────────────────

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct AdminUserResponse {
    pub id: Uuid,
    pub name: String,
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub avatar_path: Option<String>,
    pub is_admin: bool,
    pub last_login_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Serialize)]
pub struct UsersPage {
    pub items: Vec<AdminUserResponse>,
    pub total: i64,
    pub page: u32,
    pub per_page: u32,
}

#[derive(Debug, Deserialize)]
pub struct ListUsersQuery {
    pub page: Option<u32>,
    pub per_page: Option<u32>,
    pub search: Option<String>,
}

pub async fn list_users(
    State(state): State<AppState>,
    auth: AuthUser,
    axum::extract::Query(query): axum::extract::Query<ListUsersQuery>,
) -> AppResult<Json<UsersPage>> {
    guard_admin(&auth)?;

    let page = query.page.unwrap_or(1).max(1);
    let per_page = query.per_page.unwrap_or(30).clamp(1, 100);
    let offset = (page - 1) * per_page;

    let (where_clause, search_param) = if let Some(ref search) = query.search {
        let pattern = format!("%{}%", search);
        ("WHERE name ILIKE $1 OR email ILIKE $1", Some(pattern))
    } else {
        ("", None)
    };

    // Count total
    let count_sql = format!("SELECT COUNT(*) as count FROM users {}", where_clause);
    let total: i64 = if let Some(ref search) = search_param {
        sqlx::query_scalar(&count_sql)
            .bind(search)
            .fetch_one(&state.pool)
            .await?
    } else {
        sqlx::query_scalar(&count_sql)
            .fetch_one(&state.pool)
            .await?
    };

    // Fetch users
    let fetch_sql = format!(
        "SELECT id, name, email, display_name, avatar_path, is_admin, last_login_at, created_at 
         FROM users {} ORDER BY created_at DESC LIMIT $1 OFFSET $2",
        where_clause
    );

    let users: Vec<AdminUserResponse> = if let Some(ref search) = search_param {
        sqlx::query_as::<_, AdminUserResponse>(&fetch_sql)
            .bind(search)
            .bind(per_page as i64)
            .bind(offset as i64)
            .fetch_all(&state.pool)
            .await?
    } else {
        sqlx::query_as::<_, AdminUserResponse>(&fetch_sql)
            .bind(per_page as i64)
            .bind(offset as i64)
            .fetch_all(&state.pool)
            .await?
    };

    Ok(Json(UsersPage {
        items: users,
        total,
        page,
        per_page,
    }))
}

#[derive(Debug, Deserialize)]
pub struct CreateUserRequest {
    pub name: String,
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub password: String,
    pub is_admin: Option<bool>,
}

pub async fn create_user(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<CreateUserRequest>,
) -> AppResult<Json<AdminUserResponse>> {
    guard_admin(&auth)?;

    if req.name.trim().is_empty() || req.password.trim().is_empty() {
        return Err(AppError::bad_request("用户名和密码不能为空"));
    }

    if req.password.len() < 6 {
        return Err(AppError::bad_request("密码至少需要6个字符"));
    }

    let password_hash = bcrypt::hash(&req.password, bcrypt::DEFAULT_COST)?;
    let display_name = req.display_name.as_ref().unwrap_or(&req.name).to_string();
    let is_admin = req.is_admin.unwrap_or(false);

    let user = sqlx::query_as::<_, AdminUserResponse>(
        r#"
        INSERT INTO users (name, email, display_name, password_hash, is_admin)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING id, name, email, display_name, avatar_path, is_admin, last_login_at, created_at
        "#,
    )
    .bind(req.name.trim())
    .bind(&req.email)
    .bind(&display_name)
    .bind(&password_hash)
    .bind(is_admin)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| {
        if e.to_string().contains("unique") {
            AppError::bad_request("用户名或邮箱已存在")
        } else {
            AppError::internal(&e.to_string())
        }
    })?;

    // Log audit
    let _ = crate::audit::log_audit(
        &state.pool,
        Some(auth.user_id),
        Some(user.id),
        "create_user",
        Some(serde_json::json!({ 
            "name": req.name,
            "is_admin": is_admin 
        })),
        None,
    )
    .await;

    Ok(Json(user))
}

#[derive(Debug, Deserialize)]
pub struct UpdateUserRequest {
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub is_admin: Option<bool>,
}

pub async fn update_user(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(user_id): Path<Uuid>,
    Json(req): Json<UpdateUserRequest>,
) -> AppResult<Json<AdminUserResponse>> {
    guard_admin(&auth)?;

    // Build dynamic UPDATE query
    let mut updates = Vec::new();
    let mut bind_idx = 1;

    if req.email.is_some() {
        updates.push(format!("email = ${}", bind_idx));
        bind_idx += 1;
    }
    if req.display_name.is_some() {
        updates.push(format!("display_name = ${}", bind_idx));
        bind_idx += 1;
    }
    if req.is_admin.is_some() {
        updates.push(format!("is_admin = ${}", bind_idx));
        bind_idx += 1;
    }

    if updates.is_empty() {
        return Err(AppError::bad_request("没有可更新的字段"));
    }

    let update_sql = format!(
        "UPDATE users SET {} WHERE id = ${} RETURNING id, name, email, display_name, avatar_path, is_admin, last_login_at, created_at",
        updates.join(", "),
        bind_idx
    );

    let mut query = sqlx::query_as::<_, AdminUserResponse>(&update_sql);

    if let Some(ref email) = req.email {
        query = query.bind(email);
    }
    if let Some(ref display_name) = req.display_name {
        query = query.bind(display_name);
    }
    if let Some(is_admin) = req.is_admin {
        query = query.bind(is_admin);
    }
    query = query.bind(user_id);

    let user = query
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AppError::not_found("用户不存在"))?;

    // Log audit
    let _ = crate::audit::log_audit(
        &state.pool,
        Some(auth.user_id),
        Some(user_id),
        "update_user",
        Some(serde_json::json!({ 
            "email": req.email,
            "display_name": req.display_name,
            "is_admin": req.is_admin 
        })),
        None,
    )
    .await;

    Ok(Json(user))
}

pub async fn delete_user(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(user_id): Path<Uuid>,
) -> AppResult<Json<serde_json::Value>> {
    guard_admin(&auth)?;

    // Prevent self-deletion
    if user_id == auth.user_id {
        return Err(AppError::bad_request("不能删除自己的账户"));
    }

    // Get user info for audit log
    let user_name: Option<String> = sqlx::query_scalar("SELECT name FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_optional(&state.pool)
        .await?;

    // Get avatar path to delete file
    let avatar_path: Option<String> = sqlx::query_scalar("SELECT avatar_path FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_optional(&state.pool)
        .await?
        .flatten();

    // Delete user (cascade will delete sessions, conversations, messages)
    let res = sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(user_id)
        .execute(&state.pool)
        .await?;

    if res.rows_affected() == 0 {
        return Err(AppError::not_found("用户不存在"));
    }

    // Delete avatar file if exists
    if let Some(avatar_path) = avatar_path {
        let file_path = std::path::PathBuf::from(&state.artifacts_path).join(&avatar_path);
        let _ = tokio::fs::remove_file(file_path).await;
    }

    // Log audit
    let _ = crate::audit::log_audit(
        &state.pool,
        Some(auth.user_id),
        Some(user_id),
        "delete_user",
        Some(serde_json::json!({ "name": user_name })),
        None,
    )
    .await;

    Ok(Json(serde_json::json!({"ok": true})))
}

#[derive(Debug, Deserialize)]
pub struct ResetPasswordRequest {
    pub new_password: String,
}

pub async fn reset_user_password(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(user_id): Path<Uuid>,
    Json(req): Json<ResetPasswordRequest>,
) -> AppResult<Json<serde_json::Value>> {
    guard_admin(&auth)?;

    if req.new_password.trim().len() < 6 {
        return Err(AppError::bad_request("新密码至少需要6个字符"));
    }

    let new_hash = bcrypt::hash(&req.new_password, bcrypt::DEFAULT_COST)?;

    let res = sqlx::query("UPDATE users SET password_hash = $1 WHERE id = $2")
        .bind(&new_hash)
        .bind(user_id)
        .execute(&state.pool)
        .await?;

    if res.rows_affected() == 0 {
        return Err(AppError::not_found("用户不存在"));
    }

    // Log audit
    let _ = crate::audit::log_audit(
        &state.pool,
        Some(auth.user_id),
        Some(user_id),
        "reset_password",
        None,
        None,
    )
    .await;

    Ok(Json(serde_json::json!({"message": "密码重置成功"})))
}

// ── Audit Logs ───────────────────────────────────────────────────────────────

pub async fn list_audit_logs(
    State(state): State<AppState>,
    auth: AuthUser,
    axum::extract::Query(query): axum::extract::Query<crate::audit::AuditLogQuery>,
) -> AppResult<Json<crate::audit::AuditLogPage>> {
    guard_admin(&auth)?;

    let page = crate::audit::query_audit_logs(&state.pool, query)
        .await
        .map_err(|e| AppError::internal(&e.to_string()))?;

    Ok(Json(page))
}

// ── Global MCPs ──────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CreateGlobalMcpRequest {
    pub name: String,
    pub r#type: String,
    pub config: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub struct UpdateGlobalMcpRequest {
    pub name: Option<String>,
    pub r#type: Option<String>,
    pub config: Option<serde_json::Value>,
}

pub async fn list_global_mcps(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<Vec<crate::config::GlobalMcp>>> {
    guard_admin(&auth)?;
    let rows = sqlx::query_as::<_, crate::config::GlobalMcp>(
        "SELECT id, name, type, config, created_at FROM global_mcps ORDER BY created_at ASC",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

pub async fn create_global_mcp(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<CreateGlobalMcpRequest>,
) -> AppResult<Json<crate::config::GlobalMcp>> {
    guard_admin(&auth)?;

    // Validate config based on type
    if req.r#type != "http" && req.r#type != "stdio" {
        return Err(AppError::bad_request("Type must be 'http' or 'stdio'"));
    }

    let row = sqlx::query_as::<_, crate::config::GlobalMcp>(
        r#"
        INSERT INTO global_mcps (name, type, config)
        VALUES ($1, $2, $3)
        RETURNING id, name, type, config, created_at
        "#,
    )
    .bind(&req.name)
    .bind(&req.r#type)
    .bind(&req.config)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| {
        if e.to_string().contains("unique") {
            AppError::bad_request("Global MCP with this name already exists")
        } else {
            AppError::internal(&e.to_string())
        }
    })?;

    // Hot-reload: initialize the MCP connection and add it to the in-process global list.
    let mcp_cfg = mcp_config_from_req(&req.name, &req.r#type, &req.config);
    if let Some(tool) = try_init_mcp(&mcp_cfg).await {
        let mut guard = state.mcp_tools.lock().await;
        guard.retain(|(n, _)| n != &req.name);
        guard.push((req.name.clone(), tool));
    }

    Ok(Json(row))
}

pub async fn update_global_mcp(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateGlobalMcpRequest>,
) -> AppResult<Json<crate::config::GlobalMcp>> {
    guard_admin(&auth)?;

    let mut updates = Vec::new();
    let mut bind_idx = 1;

    if req.name.is_some() {
        updates.push(format!("name = ${}", bind_idx));
        bind_idx += 1;
    }
    if req.r#type.is_some() {
        updates.push(format!("type = ${}", bind_idx));
        bind_idx += 1;
    }
    if req.config.is_some() {
        updates.push(format!("config = ${}", bind_idx));
        bind_idx += 1;
    }

    if updates.is_empty() {
        return Err(AppError::bad_request("No fields to update"));
    }

    let sql = format!(
        "UPDATE global_mcps SET {} WHERE id = ${} RETURNING id, name, type, config, created_at",
        updates.join(", "),
        bind_idx
    );

    let mut query = sqlx::query_as::<_, crate::config::GlobalMcp>(&sql);
    
    if let Some(ref name) = req.name { query = query.bind(name); }
    if let Some(ref t) = req.r#type { query = query.bind(t); }
    if let Some(ref c) = req.config { query = query.bind(c); }
    
    query = query.bind(id);

    let row = query.fetch_optional(&state.pool).await?.ok_or_else(|| AppError::not_found("Global MCP not found"))?;

    // Hot-reload: re-initialize the MCP connection with updated config.
    let effective_name = row.name.clone();
    let effective_type = row.r#type.clone();
    let effective_config = row.config.clone();
    let mcp_cfg = mcp_config_from_req(&effective_name, &effective_type, &effective_config);
    if let Some(tool) = try_init_mcp(&mcp_cfg).await {
        let mut guard = state.mcp_tools.lock().await;
        guard.retain(|(n, _)| n != &effective_name);
        guard.push((effective_name, tool));
    }

    Ok(Json(row))
}

pub async fn delete_global_mcp(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<serde_json::Value>> {
    guard_admin(&auth)?;

    // Fetch the name before deleting so we can remove it from the in-process list.
    let name: Option<String> = sqlx::query_scalar("SELECT name FROM global_mcps WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.pool)
        .await?;

    let res = sqlx::query("DELETE FROM global_mcps WHERE id = $1")
        .bind(id)
        .execute(&state.pool)
        .await?;

    if res.rows_affected() == 0 {
        return Err(AppError::not_found("Global MCP not found"));
    }

    // Hot-reload: remove from the in-process global list.
    if let Some(n) = name {
        let mut guard = state.mcp_tools.lock().await;
        guard.retain(|(existing, _)| existing != &n);
    }

    Ok(Json(serde_json::json!({"ok": true})))
}

// ── helpers ───────────────────────────────────────────────────────────────────

fn mcp_config_from_req(
    name: &str,
    mcp_type: &str,
    config: &serde_json::Value,
) -> McpServerConfig {
    match mcp_type {
        "http" => {
            let url = config
                .get("url")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            McpServerConfig::Http { name: name.to_string(), url }
        }
        _ => {
            let command = config
                .get("command")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let args: Vec<String> = config
                .get("args")
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
                .unwrap_or_default();
            let env = config
                .get("env")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();
            McpServerConfig::Studio { name: name.to_string(), command, args, env }
        }
    }
}

async fn try_init_mcp(cfg: &McpServerConfig) -> Option<McpTool> {
    match cfg {
        McpServerConfig::Http { name, url } => {
            match tokio::time::timeout(Duration::from_secs(15), McpTool::http(url)).await {
                Ok(Ok(t)) => {
                    tracing::info!("global MCP '{}' hot-reloaded ({} tools)", name, t.raw_tools().len());
                    Some(t)
                }
                Ok(Err(e)) => { warn!("global MCP '{}' hot-reload failed: {e}", name); None }
                Err(_) => { warn!("global MCP '{}' hot-reload timed out", name); None }
            }
        }
        McpServerConfig::Studio { name, command, args, env } => {
            use std::collections::HashMap;
            let env_map: &HashMap<String, String> = env;
            let args_ref: Vec<&str> = args.iter().map(String::as_str).collect();
            let (cmd, wrapped_args) = if env_map.is_empty() {
                (command.as_str().to_string(), args_ref.iter().map(|s| s.to_string()).collect::<Vec<_>>())
            } else {
                let env_args: Vec<String> = env_map
                    .iter()
                    .map(|(k, v)| format!("{k}={v}"))
                    .chain(std::iter::once(command.clone()))
                    .chain(args.iter().cloned())
                    .collect();
                ("env".to_string(), env_args)
            };
            let wrapped_refs: Vec<&str> = wrapped_args.iter().map(String::as_str).collect();
            match tokio::time::timeout(Duration::from_secs(300), McpTool::stdio(&cmd, &wrapped_refs)).await {
                Ok(Ok(t)) => {
                    tracing::info!("global MCP '{}' hot-reloaded ({} tools)", name, t.raw_tools().len());
                    Some(t)
                }
                Ok(Err(e)) => { warn!("global MCP '{}' hot-reload failed: {e}", name); None }
                Err(_) => { warn!("global MCP '{}' hot-reload timed out", name); None }
            }
        }
    }
}
