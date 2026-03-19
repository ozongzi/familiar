use axum::{Json, extract::State};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::web::AppState;
use crate::web::auth::AuthUser;

#[derive(Deserialize)]
pub struct RegisterRequest {
    pub name: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct UserResponse {
    pub id: Uuid,
    pub name: String,
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub avatar_path: Option<String>,
    pub is_admin: bool,
    pub last_login_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

pub async fn register(
    State(state): State<AppState>,
    Json(req): Json<RegisterRequest>,
) -> AppResult<Json<UserResponse>> {
    if req.name.trim().is_empty() || req.password.trim().is_empty() {
        return Err(AppError::bad_request("用户名和密码不能为空"));
    }

    let password_hash = bcrypt::hash(&req.password, bcrypt::DEFAULT_COST)?;

    let row = sqlx::query(
        r#"
        INSERT INTO users (name, password_hash, display_name)
        VALUES ($1, $2, $1)
        RETURNING id, name, email, display_name, avatar_path, is_admin, last_login_at, created_at
        "#,
    )
    .bind(req.name.trim())
    .bind(password_hash)
    .fetch_one(&state.pool)
    .await?;

    let user_id: Uuid = row.try_get("id").map_err(|_| AppError::internal("db error"))?;

    // Log registration audit
    let _ = crate::audit::log_audit(
        &state.pool,
        Some(user_id),
        Some(user_id),
        "register",
        Some(serde_json::json!({ "name": req.name.trim() })),
        None,
    )
    .await;

    Ok(Json(UserResponse {
        id: user_id,
        name: row.try_get("name").map_err(|_| AppError::internal("db error"))?,
        email: row.try_get("email").ok(),
        display_name: row.try_get("display_name").ok(),
        avatar_path: row.try_get("avatar_path").ok(),
        is_admin: row.try_get("is_admin").map_err(|_| AppError::internal("db error"))?,
        last_login_at: row.try_get("last_login_at").ok(),
        created_at: row.try_get("created_at").map_err(|_| AppError::internal("db error"))?,
    }))
}

pub async fn get_me(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<UserResponse>> {
    let row = sqlx::query("SELECT id, name, email, display_name, avatar_path, is_admin, last_login_at, created_at FROM users WHERE id = $1")
        .bind(auth.user_id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AppError::not_found("用户不存在"))?;

    Ok(Json(UserResponse {
        id: row.try_get("id").map_err(|_| AppError::internal("db error"))?,
        name: row.try_get("name").map_err(|_| AppError::internal("db error"))?,
        email: row.try_get("email").ok(),
        display_name: row.try_get("display_name").ok(),
        avatar_path: row.try_get("avatar_path").ok(),
        is_admin: row.try_get("is_admin").map_err(|_| AppError::internal("db error"))?,
        last_login_at: row.try_get("last_login_at").ok(),
        created_at: row.try_get("created_at").map_err(|_| AppError::internal("db error"))?,
    }))
}

#[derive(Deserialize)]
pub struct UpdateProfileRequest {
    pub email: Option<String>,
    pub display_name: Option<String>,
}

pub async fn update_profile(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<UpdateProfileRequest>,
) -> AppResult<Json<UserResponse>> {
    // Validate email format if provided
    if let Some(ref email) = req.email
        && !email.is_empty() && !email.contains('@') {
            return Err(AppError::bad_request("邮箱格式无效"));
        }

    // Update profile
    sqlx::query(
        r#"
        UPDATE users 
        SET email = COALESCE($1, email),
            display_name = COALESCE($2, display_name)
        WHERE id = $3
        "#,
    )
    .bind(&req.email)
    .bind(&req.display_name)
    .bind(auth.user_id)
    .execute(&state.pool)
    .await?;

    // Log audit
    let _ = crate::audit::log_audit(
        &state.pool,
        Some(auth.user_id),
        Some(auth.user_id),
        "update_profile",
        Some(serde_json::json!({ 
            "email": req.email,
            "display_name": req.display_name 
        })),
        None,
    )
    .await;

    // Return updated user
    get_me(State(state), auth).await
}

#[derive(Deserialize)]
pub struct UpdatePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

pub async fn update_password(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<UpdatePasswordRequest>,
) -> AppResult<Json<serde_json::Value>> {
    if req.new_password.trim().len() < 6 {
        return Err(AppError::bad_request("新密码至少需要6个字符"));
    }

    // Verify current password
    let password_hash: String = sqlx::query_scalar("SELECT password_hash FROM users WHERE id = $1")
        .bind(auth.user_id)
        .fetch_one(&state.pool)
        .await?;

    let valid = bcrypt::verify(&req.current_password, &password_hash)?;
    if !valid {
        return Err(AppError::bad_request("当前密码错误"));
    }

    // Hash new password
    let new_hash = bcrypt::hash(&req.new_password, bcrypt::DEFAULT_COST)?;

    // Update password
    sqlx::query("UPDATE users SET password_hash = $1 WHERE id = $2")
        .bind(&new_hash)
        .bind(auth.user_id)
        .execute(&state.pool)
        .await?;

    // Log audit
    let _ = crate::audit::log_audit(
        &state.pool,
        Some(auth.user_id),
        Some(auth.user_id),
        "update_password",
        None,
        None,
    )
    .await;

    Ok(Json(serde_json::json!({ "message": "密码修改成功" })))
}
