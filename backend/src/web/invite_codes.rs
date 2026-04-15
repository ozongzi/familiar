use axum::{
    Json,
    extract::{Path, State},
};
use serde::{Deserialize, Serialize};

use crate::errors::{AppError, AppResult};
use crate::web::{AppState, auth::AuthUser};

// ── Shared types ──────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct InviteCode {
    pub code: String,
    pub created_by: uuid::Uuid,
    pub used_by: Option<uuid::Uuid>,
    pub used_at: Option<chrono::DateTime<chrono::Utc>>,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

// ── Admin: list ───────────────────────────────────────────────────────────────

pub async fn list_invite_codes(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<Vec<InviteCode>>> {
    if !auth.is_admin {
        return Err(AppError::forbidden("仅管理员可访问"));
    }
    let codes = sqlx::query_as::<_, InviteCode>(
        "SELECT code, created_by, used_by, used_at, expires_at, created_at
         FROM invite_codes ORDER BY created_at DESC",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(codes))
}

// ── Admin: create ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateInviteCodeRequest {
    /// Optional expiry in days from now. None = never expires.
    pub expires_in_days: Option<i64>,
}

pub async fn create_invite_code(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<CreateInviteCodeRequest>,
) -> AppResult<Json<InviteCode>> {
    if !auth.is_admin {
        return Err(AppError::forbidden("仅管理员可访问"));
    }

    let code = generate_code();
    let expires_at = req.expires_in_days.map(|d| {
        chrono::Utc::now() + chrono::Duration::days(d)
    });

    let row = sqlx::query_as::<_, InviteCode>(
        "INSERT INTO invite_codes (code, created_by, expires_at)
         VALUES ($1, $2, $3)
         RETURNING code, created_by, used_by, used_at, expires_at, created_at",
    )
    .bind(&code)
    .bind(auth.user_id)
    .bind(expires_at)
    .fetch_one(&state.pool)
    .await?;

    Ok(Json(row))
}

// ── Admin: revoke ─────────────────────────────────────────────────────────────

pub async fn delete_invite_code(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(code): Path<String>,
) -> AppResult<Json<serde_json::Value>> {
    if !auth.is_admin {
        return Err(AppError::forbidden("仅管理员可访问"));
    }

    let result = sqlx::query(
        "DELETE FROM invite_codes WHERE code = $1 AND used_by IS NULL",
    )
    .bind(&code)
    .execute(&state.pool)
    .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::bad_request("邀请码不存在或已被使用"));
    }

    Ok(Json(serde_json::json!({ "ok": true })))
}

// ── Public: register with invite code ────────────────────────────────────────

#[derive(Deserialize)]
pub struct RegisterRequest {
    pub name: String,
    pub password: String,
    pub invite_code: String,
}

#[derive(Serialize)]
pub struct RegisterResponse {
    pub token: String,
}

pub async fn register_with_invite(
    State(state): State<AppState>,
    Json(req): Json<RegisterRequest>,
) -> AppResult<Json<RegisterResponse>> {
    // Validate invite code
    let code_row = sqlx::query(
        "SELECT code, expires_at FROM invite_codes
         WHERE code = $1 AND used_by IS NULL",
    )
    .bind(&req.invite_code)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::bad_request("无效的邀请码"))?;

    use sqlx::Row;
    let expires_at: Option<chrono::DateTime<chrono::Utc>> =
        code_row.try_get("expires_at").unwrap_or(None);
    if let Some(exp) = expires_at
        && exp < chrono::Utc::now() {
            return Err(AppError::bad_request("邀请码已过期"));
        }

    // Validate username
    let name = req.name.trim().to_string();
    if name.is_empty() || name.len() > 32 {
        return Err(AppError::bad_request("用户名长度须在 1–32 个字符之间"));
    }
    if name.chars().any(|c| !c.is_alphanumeric() && c != '_' && c != '-') {
        return Err(AppError::bad_request("用户名只能包含字母、数字、_ 和 -"));
    }

    // Validate password
    if req.password.len() < 8 {
        return Err(AppError::bad_request("密码长度至少 8 位"));
    }

    let hash = bcrypt::hash(&req.password, bcrypt::DEFAULT_COST)
        .map_err(|e| AppError::internal(&e.to_string()))?;

    let invite_code_for_user = crate::web::github_oauth::gen_invite_code();

    let user_id = sqlx::query_scalar::<_, uuid::Uuid>(
        "INSERT INTO users (name, password_hash, display_name, invite_code)
         VALUES ($1, $2, $1, $3)
         RETURNING id",
    )
    .bind(&name)
    .bind(&hash)
    .bind(&invite_code_for_user)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| {
        if e.to_string().contains("unique") || e.to_string().contains("duplicate") {
            AppError::bad_request("用户名已被占用")
        } else {
            AppError::internal(&e.to_string())
        }
    })?;

    // Mark invite code as used
    sqlx::query(
        "UPDATE invite_codes SET used_by = $1, used_at = NOW() WHERE code = $2",
    )
    .bind(user_id)
    .bind(&req.invite_code)
    .execute(&state.pool)
    .await?;

    // Create session
    let token = gen_token();
    sqlx::query(
        "INSERT INTO sessions (token, user_id, expires_at)
         VALUES ($1, $2, NOW() + INTERVAL '30 days')",
    )
    .bind(&token)
    .bind(user_id)
    .execute(&state.pool)
    .await?;


    let _ = crate::audit::log_audit(
        &state.pool,
        Some(user_id),
        None,
        "register_invite",
        Some(serde_json::json!({ "name": name, "invite_code": req.invite_code })),
        None,
    )
    .await;

    Ok(Json(RegisterResponse { token }))
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn generate_code() -> String {
    use std::fmt::Write;
    let mut bytes = [0u8; 8];
    getrandom::getrandom(&mut bytes).expect("getrandom failed");
    let mut s = String::with_capacity(16);
    for b in bytes {
        write!(s, "{b:02x}").unwrap();
    }
    s
}

fn gen_token() -> String {
    use std::fmt::Write;
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes).expect("getrandom failed");
    let mut s = String::with_capacity(64);
    for b in bytes {
        write!(s, "{b:02x}").unwrap();
    }
    s
}
