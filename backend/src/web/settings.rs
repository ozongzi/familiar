use axum::{
    Json,
    extract::{Path, State},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::web::AppState;
use crate::web::auth::AuthUser;

#[derive(Debug, Serialize, Deserialize)]
pub struct UserSettingsResponse {
    pub frontier_model: Value,
    pub cheap_model: Value,
    pub system_prompt: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateSettingsRequest {
    pub frontier_model: Option<Value>,
    pub cheap_model: Option<Value>,
    pub system_prompt: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct UserSkill {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub content: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateSkillRequest {
    pub name: String,
    pub description: Option<String>,
    pub content: String,
}

// #[derive(sqlx::FromRow)]
// struct SettingsRow {
//     frontier_model: Option<Value>,
//     cheap_model: Option<Option<Value>>, // sqlx might wrap Option<Value> as Option<Option<Value>> if it's nullable
//     system_prompt: Option<String>,
// }

// Actually, let's just use manual fetching for simplicity or correct struct
#[derive(sqlx::FromRow)]
struct SimpleSettingsRow {
    frontier_model: Option<Value>,
    cheap_model: Option<Value>,
    system_prompt: Option<String>,
}

pub async fn get_settings(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<UserSettingsResponse>> {
    let row = sqlx::query_as::<_, SimpleSettingsRow>(
        "SELECT frontier_model, cheap_model, system_prompt FROM user_settings WHERE user_id = $1",
    )
    .bind(auth.user_id)
    .fetch_optional(&state.pool)
    .await?;

    let (frontier, cheap, prompt) = if let Some(r) = row {
        (
            r.frontier_model
                .unwrap_or(serde_json::to_value(&state.frontier_model).unwrap()),
            r.cheap_model
                .unwrap_or(serde_json::to_value(&state.cheap_model).unwrap()),
            r.system_prompt.or(state.system_prompt.clone()),
        )
    } else {
        (
            serde_json::to_value(&state.frontier_model).unwrap(),
            serde_json::to_value(&state.cheap_model).unwrap(),
            state.system_prompt.clone(),
        )
    };

    Ok(Json(UserSettingsResponse {
        frontier_model: frontier,
        cheap_model: cheap,
        system_prompt: prompt,
    }))
}

pub async fn update_settings(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<UpdateSettingsRequest>,
) -> AppResult<Json<Value>> {
    sqlx::query(
        r#"
        INSERT INTO user_settings (user_id, frontier_model, cheap_model, system_prompt, updated_at)
        VALUES ($1, $2, $3, $4, NOW())
        ON CONFLICT (user_id) DO UPDATE SET
            frontier_model = COALESCE(EXCLUDED.frontier_model, user_settings.frontier_model),
            cheap_model = COALESCE(EXCLUDED.cheap_model, user_settings.cheap_model),
            system_prompt = COALESCE(EXCLUDED.system_prompt, user_settings.system_prompt),
            updated_at = NOW()
        "#,
    )
    .bind(auth.user_id)
    .bind(&req.frontier_model)
    .bind(&req.cheap_model)
    .bind(&req.system_prompt)
    .execute(&state.pool)
    .await?;

    Ok(Json(serde_json::json!({ "ok": true })))
}

// ── Skills API ──────────────────────────────────────────────────────────────

pub async fn list_skills(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<Vec<UserSkill>>> {
    let rows = sqlx::query_as::<_, UserSkill>(
        "SELECT id, name, description, content, created_at FROM user_skills WHERE user_id = $1 ORDER BY name ASC"
    )
    .bind(auth.user_id)
    .fetch_all(&state.pool)
    .await?;

    Ok(Json(rows))
}

pub async fn create_skill(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<CreateSkillRequest>,
) -> AppResult<Json<UserSkill>> {
    let row = sqlx::query_as::<_, UserSkill>(
        r#"
        INSERT INTO user_skills (user_id, name, description, content)
        VALUES ($1, $2, $3, $4)
        RETURNING id, name, description, content, created_at
        "#,
    )
    .bind(auth.user_id)
    .bind(&req.name)
    .bind(&req.description)
    .bind(&req.content)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| {
        if e.to_string().contains("unique") {
            AppError::bad_request("已存在同名 Skill")
        } else {
            AppError::internal(&e.to_string())
        }
    })?;

    Ok(Json(row))
}

pub async fn update_skill(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<CreateSkillRequest>,
) -> AppResult<Json<UserSkill>> {
    let row = sqlx::query_as::<_, UserSkill>(
        r#"
        UPDATE user_skills
        SET name = $1, description = $2, content = $3, updated_at = NOW()
        WHERE id = $4 AND user_id = $5
        RETURNING id, name, description, content, created_at
        "#,
    )
    .bind(&req.name)
    .bind(&req.description)
    .bind(&req.content)
    .bind(id)
    .bind(auth.user_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::not_found("Skill 不存在"))?;

    Ok(Json(row))
}

pub async fn delete_skill(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Value>> {
    let res = sqlx::query("DELETE FROM user_skills WHERE id = $1 AND user_id = $2")
        .bind(id)
        .bind(auth.user_id)
        .execute(&state.pool)
        .await?;

    if res.rows_affected() == 0 {
        return Err(AppError::not_found("Skill 不存在"));
    }

    Ok(Json(serde_json::json!({ "ok": true })))
}
