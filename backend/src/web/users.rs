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
    pub is_admin: bool,
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
        INSERT INTO users (name, password_hash)
        VALUES ($1, $2)
        RETURNING id, name, is_admin
        "#,
    )
    .bind(req.name.trim())
    .bind(password_hash)
    .fetch_one(&state.pool)
    .await?;

    Ok(Json(UserResponse {
        id: row
            .try_get("id")
            .map_err(|_| AppError::internal("db error"))?,
        name: row
            .try_get("name")
            .map_err(|_| AppError::internal("db error"))?,
        is_admin: row
            .try_get("is_admin")
            .map_err(|_| AppError::internal("db error"))?,
    }))
}

pub async fn get_me(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<UserResponse>> {
    let row = sqlx::query("SELECT id, name, is_admin FROM users WHERE id = $1")
        .bind(auth.user_id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AppError::not_found("用户不存在"))?;

    Ok(Json(UserResponse {
        id: row
            .try_get("id")
            .map_err(|_| AppError::internal("db error"))?,
        name: row
            .try_get("name")
            .map_err(|_| AppError::internal("db error"))?,
        is_admin: row
            .try_get("is_admin")
            .map_err(|_| AppError::internal("db error"))?,
    }))
}
