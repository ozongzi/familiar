use axum::{
    Json,
    extract::State,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::errors::AppResult;
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

#[derive(sqlx::FromRow)]
struct SettingsRow {
    frontier_model: Option<Value>,
    cheap_model: Option<Option<Value>>, // sqlx might wrap Option<Value> as Option<Option<Value>> if it's nullable
    system_prompt: Option<String>,
}

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
        "SELECT frontier_model, cheap_model, system_prompt FROM user_settings WHERE user_id = $1"
    )
    .bind(auth.user_id)
    .fetch_optional(&state.pool)
    .await?;

    let (frontier, cheap, prompt) = if let Some(r) = row {
        (
            r.frontier_model.unwrap_or(serde_json::to_value(&state.frontier_model).unwrap()),
            r.cheap_model.unwrap_or(serde_json::to_value(&state.cheap_model).unwrap()),
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
        "#
    )
    .bind(auth.user_id)
    .bind(&req.frontier_model)
    .bind(&req.cheap_model)
    .bind(&req.system_prompt)
    .execute(&state.pool)
    .await?;

    Ok(Json(serde_json::json!({ "ok": true })))
}
