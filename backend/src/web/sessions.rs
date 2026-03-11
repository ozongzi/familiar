use axum::{Json, extract::State};
use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::errors::{AppError, AppResult};
use crate::web::AppState;
use crate::web::auth::AuthUser;

#[derive(Deserialize)]
pub struct LoginRequest {
    pub name: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct LoginResponse {
    pub token: String,
}

pub async fn login(
    State(state): State<AppState>,
    Json(req): Json<LoginRequest>,
) -> AppResult<Json<LoginResponse>> {
    let row = sqlx::query("SELECT id, password_hash FROM users WHERE name = $1")
        .bind(&req.name)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AppError::bad_request("用户名或密码错误"))?;

    let id: uuid::Uuid = row
        .try_get("id")
        .map_err(|_| AppError::internal("db error"))?;
    let password_hash: String = row
        .try_get("password_hash")
        .map_err(|_| AppError::internal("db error"))?;

    let valid = bcrypt::verify(&req.password, &password_hash)?;
    if !valid {
        return Err(AppError::bad_request("用户名或密码错误"));
    }

    let token = generate_token();

    sqlx::query("INSERT INTO sessions (token, user_id) VALUES ($1, $2)")
        .bind(&token)
        .bind(id)
        .execute(&state.pool)
        .await?;

    Ok(Json(LoginResponse { token }))
}

pub async fn logout(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<serde_json::Value>> {
    sqlx::query("DELETE FROM sessions WHERE user_id = $1")
        .bind(auth.user_id)
        .execute(&state.pool)
        .await?;

    Ok(Json(serde_json::json!({ "ok": true })))
}

fn generate_token() -> String {
    use std::fmt::Write;
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes).expect("getrandom failed");
    let mut s = String::with_capacity(64);
    for b in bytes {
        write!(s, "{b:02x}").unwrap();
    }
    s
}
