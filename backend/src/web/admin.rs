use axum::{
    Json,
    extract::{Path, State},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::config::Config;
use crate::errors::{AppError, AppResult};
use crate::web::{AppState, auth::AuthUser};

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
    // No in-memory cache to evict — workers load config from DB each time.
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

    let conversation_ids: Vec<uuid::Uuid> =
        sqlx::query_scalar("SELECT id FROM conversations WHERE user_id = $1")
            .bind(user_id)
            .fetch_all(&state.pool)
            .await?;

    // Get avatar path to delete file
    let avatar_path: Option<String> =
        sqlx::query_scalar("SELECT avatar_path FROM users WHERE id = $1")
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

    if let Err(err) = state
        .sandbox
        .remove_user_resources(user_id, &conversation_ids)
    {
        tracing::error!(user_id = %user_id, error = %err, "failed to remove user sandbox resources");
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

    // No in-memory hot-reload needed — workers load MCPs from DB each time.

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

    if let Some(ref name) = req.name {
        query = query.bind(name);
    }
    if let Some(ref t) = req.r#type {
        query = query.bind(t);
    }
    if let Some(ref c) = req.config {
        query = query.bind(c);
    }

    query = query.bind(id);

    let row = query
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AppError::not_found("Global MCP not found"))?;

    // No in-memory hot-reload needed — workers load MCPs from DB each time.

    Ok(Json(row))
}

pub async fn delete_global_mcp(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<serde_json::Value>> {
    guard_admin(&auth)?;

    let res = sqlx::query("DELETE FROM global_mcps WHERE id = $1")
        .bind(id)
        .execute(&state.pool)
        .await?;

    if res.rows_affected() == 0 {
        return Err(AppError::not_found("Global MCP not found"));
    }

    // No in-memory hot-reload needed — workers load MCPs from DB each time.

    Ok(Json(serde_json::json!({"ok": true})))
}

/// GET /api/admin/token-usage
pub async fn get_token_usage(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<serde_json::Value>> {
    guard_admin(&auth)?;
    let row = sqlx::query(
        r#"SELECT COALESCE(SUM(prompt_tokens), 0)::bigint          AS prompt_tokens,
                  COALESCE(SUM(completion_tokens), 0)::bigint      AS completion_tokens,
                  COALESCE(SUM(total_tokens), 0)::bigint           AS total_tokens,
                  COALESCE(SUM(cache_read_tokens), 0)::bigint      AS cache_read_tokens,
                  COALESCE(SUM(cache_creation_tokens), 0)::bigint  AS cache_creation_tokens,
                  COUNT(DISTINCT conversation_id)::bigint           AS conversation_count
           FROM token_usage_events"#,
    )
    .fetch_one(&state.pool)
    .await?;

    use sqlx::Row;
    Ok(Json(serde_json::json!({
        "prompt_tokens":        row.get::<i64, _>("prompt_tokens"),
        "completion_tokens":    row.get::<i64, _>("completion_tokens"),
        "total_tokens":         row.get::<i64, _>("total_tokens"),
        "cache_read_tokens":    row.get::<i64, _>("cache_read_tokens"),
        "cache_creation_tokens":row.get::<i64, _>("cache_creation_tokens"),
        "conversation_count":   row.get::<i64, _>("conversation_count"),
    })))
}

pub async fn get_token_usage_by_user(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<serde_json::Value>> {
    guard_admin(&auth)?;
    let rows = sqlx::query(
        r#"
        SELECT u.id::text AS user_id, u.name AS username,
               COUNT(t.conversation_id)::bigint                         AS conversation_count,
               COALESCE(SUM(t.prompt_tokens), 0)::bigint                AS prompt_tokens,
               COALESCE(SUM(t.completion_tokens), 0)::bigint            AS completion_tokens,
               COALESCE(SUM(t.total_tokens), 0)::bigint                 AS total_tokens,
               COALESCE(SUM(t.cache_read_tokens), 0)::bigint            AS cache_read_tokens,
               COALESCE(SUM(t.cache_creation_tokens), 0)::bigint        AS cache_creation_tokens
        FROM users u
        LEFT JOIN token_usage_events t ON t.user_id = u.id
        GROUP BY u.id, u.name
        ORDER BY total_tokens DESC
        "#,
    )
    .fetch_all(&state.pool)
    .await?;

    let users: Vec<serde_json::Value> = rows
        .iter()
        .map(|r| {
            use sqlx::Row;
            serde_json::json!({
                "user_id":              r.get::<String, _>("user_id"),
                "username":             r.get::<String, _>("username"),
                "conversation_count":   r.get::<i64, _>("conversation_count"),
                "prompt_tokens":        r.get::<i64, _>("prompt_tokens"),
                "completion_tokens":    r.get::<i64, _>("completion_tokens"),
                "total_tokens":         r.get::<i64, _>("total_tokens"),
                "cache_read_tokens":    r.get::<i64, _>("cache_read_tokens"),
                "cache_creation_tokens":r.get::<i64, _>("cache_creation_tokens"),
            })
        })
        .collect();

    Ok(Json(serde_json::json!({ "users": users })))
}

pub async fn get_token_usage_conversations(
    State(state): State<AppState>,
    auth: AuthUser,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> AppResult<Json<serde_json::Value>> {
    guard_admin(&auth)?;
    let user_id = params.get("user_id").cloned().unwrap_or_default();

    let rows = if user_id.is_empty() {
        sqlx::query(
            r#"
            SELECT t.conversation_id::text AS conv_id,
                   COALESCE(MAX(t.conversation_name), '(deleted)') AS conv_name,
                   u.name AS username,
                   TO_TIMESTAMP(MAX(t.created_at))::text AS created_at,
                   COALESCE(SUM(t.prompt_tokens), 0)::bigint AS prompt_tokens,
                   COALESCE(SUM(t.completion_tokens), 0)::bigint AS completion_tokens,
                   COALESCE(SUM(t.total_tokens), 0)::bigint AS total_tokens,
                   COALESCE(SUM(t.cache_read_tokens), 0)::bigint AS cache_read_tokens,
                   COALESCE(SUM(t.cache_creation_tokens), 0)::bigint AS cache_creation_tokens
            FROM token_usage_events t
            JOIN users u ON u.id = t.user_id
            GROUP BY t.conversation_id, u.name
            HAVING COALESCE(SUM(t.total_tokens), 0) > 0
            ORDER BY MAX(t.created_at) DESC
            LIMIT 200
            "#,
        )
        .fetch_all(&state.pool)
        .await?
    } else {
        sqlx::query(
            r#"
            SELECT t.conversation_id::text AS conv_id,
                   COALESCE(MAX(t.conversation_name), '(deleted)') AS conv_name,
                   u.name AS username,
                   TO_TIMESTAMP(MAX(t.created_at))::text AS created_at,
                   COALESCE(SUM(t.prompt_tokens), 0)::bigint AS prompt_tokens,
                   COALESCE(SUM(t.completion_tokens), 0)::bigint AS completion_tokens,
                   COALESCE(SUM(t.total_tokens), 0)::bigint AS total_tokens,
                   COALESCE(SUM(t.cache_read_tokens), 0)::bigint AS cache_read_tokens,
                   COALESCE(SUM(t.cache_creation_tokens), 0)::bigint AS cache_creation_tokens
            FROM token_usage_events t
            JOIN users u ON u.id = t.user_id
            WHERE t.user_id = $1::uuid
            GROUP BY t.conversation_id, u.name
            HAVING COALESCE(SUM(t.total_tokens), 0) > 0
            ORDER BY MAX(t.created_at) DESC
            LIMIT 200
            "#,
        )
        .bind(&user_id)
        .fetch_all(&state.pool)
        .await?
    };

    let conversations: Vec<serde_json::Value> = rows
        .iter()
        .map(|r| {
            use sqlx::Row;
            serde_json::json!({
                "conv_id":              r.get::<String, _>("conv_id"),
                "conv_name":            r.get::<String, _>("conv_name"),
                "username":             r.get::<String, _>("username"),
                "created_at":           r.get::<String, _>("created_at"),
                "prompt_tokens":        r.get::<i64, _>("prompt_tokens"),
                "completion_tokens":    r.get::<i64, _>("completion_tokens"),
                "total_tokens":         r.get::<i64, _>("total_tokens"),
                "cache_read_tokens":    r.get::<i64, _>("cache_read_tokens"),
                "cache_creation_tokens":r.get::<i64, _>("cache_creation_tokens"),
            })
        })
        .collect();

    Ok(Json(serde_json::json!({ "conversations": conversations })))
}

pub async fn get_token_usage_daily(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<serde_json::Value>> {
    guard_admin(&auth)?;
    let rows = sqlx::query(
        r#"
        SELECT DATE(TO_TIMESTAMP(recorded_at))::text                AS day,
               COALESCE(SUM(total_tokens), 0)::bigint               AS total_tokens,
               COALESCE(SUM(prompt_tokens), 0)::bigint              AS prompt_tokens,
               COALESCE(SUM(completion_tokens), 0)::bigint          AS completion_tokens,
               COALESCE(SUM(cache_read_tokens), 0)::bigint          AS cache_read_tokens,
               COALESCE(SUM(cache_creation_tokens), 0)::bigint      AS cache_creation_tokens,
               COUNT(*)::bigint                                      AS conversation_count
        FROM token_usage_events
        WHERE created_at >= EXTRACT(EPOCH FROM NOW() - INTERVAL '30 days')::bigint
          AND total_tokens > 0
        GROUP BY DATE(TO_TIMESTAMP(created_at))
        ORDER BY day ASC
        "#,
    )
    .fetch_all(&state.pool)
    .await?;

    let days: Vec<serde_json::Value> = rows
        .iter()
        .map(|r| {
            use sqlx::Row;
            serde_json::json!({
                "day":                  r.get::<String, _>("day"),
                "total_tokens":         r.get::<i64, _>("total_tokens"),
                "prompt_tokens":        r.get::<i64, _>("prompt_tokens"),
                "completion_tokens":    r.get::<i64, _>("completion_tokens"),
                "cache_read_tokens":    r.get::<i64, _>("cache_read_tokens"),
                "cache_creation_tokens":r.get::<i64, _>("cache_creation_tokens"),
                "conversation_count":   r.get::<i64, _>("conversation_count"),
            })
        })
        .collect();

    Ok(Json(serde_json::json!({ "days": days })))
}

// ── SQL panel ─────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct SqlQuery {
    pub sql: String,
}

pub async fn run_sql(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(payload): Json<SqlQuery>,
) -> AppResult<Json<serde_json::Value>> {
    guard_admin(&auth)?;

    // Audit before execution so attempts (including ones that error out) are recorded.
    let _ = crate::audit::log_audit(
        &state.pool,
        Some(auth.user_id),
        None,
        "run_sql",
        Some(serde_json::json!({ "sql": payload.sql })),
        None,
    )
    .await;

    use sqlx::Row;

    use sqlx::postgres::PgRow;
    use sqlx::{Column, TypeInfo};

    let rows: Vec<PgRow> = sqlx::query(&payload.sql)
        .fetch_all(&state.pool)
        .await
        .map_err(|e| AppError::internal(&e.to_string()))?;

    if rows.is_empty() {
        return Ok(Json(serde_json::json!({ "columns": [], "rows": [] })));
    }

    let columns: Vec<String> = rows[0]
        .columns()
        .iter()
        .map(|c| c.name().to_string())
        .collect();

    let result_rows: Vec<serde_json::Value> = rows
        .iter()
        .map(|row: &PgRow| {
            let mut obj = serde_json::Map::new();
            for col in row.columns() {
                let idx = col.ordinal();
                let type_name = col.type_info().name();
                let val: serde_json::Value = match type_name {
                    "BOOL" => row
                        .try_get::<Option<bool>, _>(idx)
                        .ok()
                        .flatten()
                        .map(serde_json::Value::Bool)
                        .unwrap_or(serde_json::Value::Null),
                    "INT2" | "INT4" => row
                        .try_get::<Option<i32>, _>(idx)
                        .ok()
                        .flatten()
                        .map(|n| serde_json::json!(n))
                        .unwrap_or(serde_json::Value::Null),
                    "INT8" | "OID" => row
                        .try_get::<Option<i64>, _>(idx)
                        .ok()
                        .flatten()
                        .map(|n| serde_json::json!(n))
                        .unwrap_or(serde_json::Value::Null),
                    "FLOAT4" | "FLOAT8" | "NUMERIC" => row
                        .try_get::<Option<f64>, _>(idx)
                        .ok()
                        .flatten()
                        .map(|n| serde_json::json!(n))
                        .unwrap_or(serde_json::Value::Null),
                    "JSONB" | "JSON" => row
                        .try_get::<Option<serde_json::Value>, _>(idx)
                        .ok()
                        .flatten()
                        .unwrap_or(serde_json::Value::Null),
                    "UUID" => row
                        .try_get::<Option<uuid::Uuid>, _>(idx)
                        .ok()
                        .flatten()
                        .map(|u| serde_json::Value::String(u.to_string()))
                        .unwrap_or(serde_json::Value::Null),
                    "TIMESTAMPTZ" => row
                        .try_get::<Option<chrono::DateTime<chrono::Utc>>, _>(idx)
                        .ok()
                        .flatten()
                        .map(|t| serde_json::Value::String(t.to_rfc3339()))
                        .unwrap_or(serde_json::Value::Null),
                    "TIMESTAMP" => row
                        .try_get::<Option<chrono::NaiveDateTime>, _>(idx)
                        .ok()
                        .flatten()
                        .map(|t| serde_json::Value::String(t.to_string()))
                        .unwrap_or(serde_json::Value::Null),
                    "DATE" => row
                        .try_get::<Option<chrono::NaiveDate>, _>(idx)
                        .ok()
                        .flatten()
                        .map(|d| serde_json::Value::String(d.to_string()))
                        .unwrap_or(serde_json::Value::Null),
                    "TIME" => row
                        .try_get::<Option<chrono::NaiveTime>, _>(idx)
                        .ok()
                        .flatten()
                        .map(|t| serde_json::Value::String(t.to_string()))
                        .unwrap_or(serde_json::Value::Null),
                    "BYTEA" => row
                        .try_get::<Option<Vec<u8>>, _>(idx)
                        .ok()
                        .flatten()
                        .map(|b| {
                            let hex: String = b.iter().map(|x| format!("{:02x}", x)).collect();
                            serde_json::Value::String(format!("\\x{}", hex))
                        })
                        .unwrap_or(serde_json::Value::Null),
                    _ => row
                        .try_get::<Option<String>, _>(idx)
                        .ok()
                        .flatten()
                        .map(serde_json::Value::String)
                        .unwrap_or(serde_json::Value::Null),
                };
                obj.insert(col.name().to_string(), val);
            }
            serde_json::Value::Object(obj)
        })
        .collect();

    Ok(Json(
        serde_json::json!({ "columns": columns, "rows": result_rows }),
    ))
}

// ── MCP Catalog ───────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct CatalogEntry {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub command: String,
    pub args: serde_json::Value,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CatalogEntryRequest {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
}

pub async fn list_catalog(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<Vec<CatalogEntry>>> {
    guard_admin(&auth)?;
    let rows =
        sqlx::query_as::<_, CatalogEntry>("SELECT * FROM mcp_catalog ORDER BY created_at ASC")
            .fetch_all(&state.pool)
            .await?;
    Ok(Json(rows))
}

pub async fn create_catalog_entry(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<CatalogEntryRequest>,
) -> AppResult<Json<CatalogEntry>> {
    guard_admin(&auth)?;
    let row = sqlx::query_as::<_, CatalogEntry>(
        "INSERT INTO mcp_catalog (name, description, command, args)
         VALUES ($1, $2, $3, $4) RETURNING *",
    )
    .bind(&req.name)
    .bind(&req.description)
    .bind(&req.command)
    .bind(serde_json::to_value(&req.args).unwrap_or_default())
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(row))
}

pub async fn update_catalog_entry(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<CatalogEntryRequest>,
) -> AppResult<Json<CatalogEntry>> {
    guard_admin(&auth)?;
    let row = sqlx::query_as::<_, CatalogEntry>(
        "UPDATE mcp_catalog SET name=$1, description=$2, command=$3, args=$4
         WHERE id=$5 RETURNING *",
    )
    .bind(&req.name)
    .bind(&req.description)
    .bind(&req.command)
    .bind(serde_json::to_value(&req.args).unwrap_or_default())
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::not_found("目录项不存在"))?;
    Ok(Json(row))
}

pub async fn delete_catalog_entry(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<serde_json::Value>> {
    guard_admin(&auth)?;
    let result = sqlx::query("DELETE FROM mcp_catalog WHERE id=$1")
        .bind(id)
        .execute(&state.pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::not_found("目录项不存在"));
    }
    Ok(Json(serde_json::json!({ "ok": true })))
}
