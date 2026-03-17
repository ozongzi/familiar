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
