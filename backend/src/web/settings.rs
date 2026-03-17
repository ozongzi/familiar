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
    pub mode: String,
    pub api_key: Option<String>,
    pub system_prompt: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateSettingsRequest {
    pub mode: String,
    pub api_key: Option<String>,
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

#[derive(sqlx::FromRow)]
struct SimpleSettingsRow {
    frontier_model: Option<Value>,
    _cheap_model: Option<Value>,
    system_prompt: Option<String>,
}

pub async fn get_settings(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<UserSettingsResponse>> {
    let row = sqlx::query_as::<_, SimpleSettingsRow>(
        "SELECT frontier_model, cheap_model as _cheap_model, system_prompt FROM user_settings WHERE user_id = $1",
    )
    .bind(auth.user_id)
    .fetch_optional(&state.pool)
    .await?;

    if let Some(r) = row {
        let api_key = r
            .frontier_model
            .as_ref()
            .and_then(|v| v.get("api_key"))
            .and_then(|v| v.as_str())
            .map(str::to_string);

        if api_key.is_some()
            && r.system_prompt
                .as_ref()
                .is_some_and(|s| !s.trim().is_empty())
        {
            return Ok(Json(UserSettingsResponse {
                mode: "custom".to_string(),
                api_key,
                system_prompt: r.system_prompt,
            }));
        }
    }

    Ok(Json(UserSettingsResponse {
        mode: "default".to_string(),
        api_key: None,
        system_prompt: None,
    }))
}

pub async fn update_settings(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<UpdateSettingsRequest>,
) -> AppResult<Json<Value>> {
    match req.mode.as_str() {
        "default" => {
            sqlx::query(
                r#"
                INSERT INTO user_settings (user_id, frontier_model, cheap_model, system_prompt, updated_at)
                VALUES ($1, NULL, NULL, NULL, NOW())
                ON CONFLICT (user_id) DO UPDATE SET
                    frontier_model = NULL,
                    cheap_model = NULL,
                    system_prompt = NULL,
                    updated_at = NOW()
                "#,
            )
            .bind(auth.user_id)
            .execute(&state.pool)
            .await?;
        }
        "custom" => {
            let api_key = req
                .api_key
                .clone()
                .filter(|s| !s.trim().is_empty())
                .ok_or_else(|| AppError::bad_request("自定义模式必须填写 API Key"))?;
            let system_prompt = req
                .system_prompt
                .clone()
                .filter(|s| !s.trim().is_empty())
                .ok_or_else(|| AppError::bad_request("自定义模式必须填写 System Prompt"))?;

            let global_cfg = state.get_global_config().await?;

            let mut frontier = global_cfg.frontier_model;
            frontier.api_key = api_key.clone();

            let mut cheap = global_cfg.cheap_model;
            cheap.api_key = api_key;

            sqlx::query(
                r#"
                INSERT INTO user_settings (user_id, frontier_model, cheap_model, system_prompt, updated_at)
                VALUES ($1, $2, $3, $4, NOW())
                ON CONFLICT (user_id) DO UPDATE SET
                    frontier_model = EXCLUDED.frontier_model,
                    cheap_model = EXCLUDED.cheap_model,
                    system_prompt = EXCLUDED.system_prompt,
                    updated_at = NOW()
                "#,
            )
            .bind(auth.user_id)
            .bind(serde_json::to_value(frontier).map_err(|e| AppError::internal(&e.to_string()))?)
            .bind(serde_json::to_value(cheap).map_err(|e| AppError::internal(&e.to_string()))?)
            .bind(system_prompt)
            .execute(&state.pool)
            .await?;
        }
        _ => return Err(AppError::bad_request("mode 必须是 custom 或 default")),
    }

    Ok(Json(serde_json::json!({ "ok": true })))
}

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
