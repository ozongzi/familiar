use axum::{
    Json,
    extract::{Path, State},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::web::AppState;
use crate::web::auth::AuthUser;

// ── Types ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ModelRow {
    pub id: Uuid,
    pub user_id: Option<Uuid>,
    pub scope: String,
    pub label: String,
    pub provider: String,
    pub model_name: String,
    pub api_base: String,
    pub api_key: String,
    pub extra_body: Value,
    pub is_default: bool,
    pub created_at: sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>,
}

#[derive(Debug, Serialize)]
pub struct ModelResponse {
    pub id: Uuid,
    pub scope: String,
    pub label: String,
    pub provider: String,
    pub model_name: String,
    pub api_base: String,
    pub is_default: bool,
    pub created_at: String,
    // api_key intentionally omitted from responses
}

impl From<ModelRow> for ModelResponse {
    fn from(r: ModelRow) -> Self {
        ModelResponse {
            id: r.id,
            scope: r.scope,
            label: r.label,
            provider: r.provider,
            model_name: r.model_name,
            api_base: r.api_base,
            is_default: r.is_default,
            created_at: r.created_at.to_rfc3339(),
        }
    }
}

#[derive(Deserialize)]
pub struct UpsertModelRequest {
    pub label: String,
    pub provider: String,
    pub model_name: String,
    #[serde(default)]
    pub api_base: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub extra_body: Value,
}

// ── User endpoints ────────────────────────────────────────────────────────────

/// List models visible to the current user: global + their own.
pub async fn list_models(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<Vec<ModelResponse>>> {
    let rows = sqlx::query_as::<_, ModelRow>(
        "SELECT * FROM models WHERE scope = 'global' OR user_id = $1 ORDER BY scope DESC, created_at ASC",
    )
    .bind(auth.user_id)
    .fetch_all(&state.pool)
    .await?;

    Ok(Json(rows.into_iter().map(ModelResponse::from).collect()))
}

/// Create a user-scoped model.
pub async fn create_model(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<UpsertModelRequest>,
) -> AppResult<Json<ModelResponse>> {
    let row = sqlx::query_as::<_, ModelRow>(
        "INSERT INTO models (user_id, scope, label, provider, model_name, api_base, api_key, extra_body)
         VALUES ($1, 'user', $2, $3, $4, $5, $6, $7)
         RETURNING *",
    )
    .bind(auth.user_id)
    .bind(&req.label)
    .bind(&req.provider)
    .bind(&req.model_name)
    .bind(&req.api_base)
    .bind(&req.api_key)
    .bind(&req.extra_body)
    .fetch_one(&state.pool)
    .await?;

    Ok(Json(ModelResponse::from(row)))
}

/// Update a user-scoped model (must belong to caller).
pub async fn update_model(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<UpsertModelRequest>,
) -> AppResult<Json<ModelResponse>> {
    let row = sqlx::query_as::<_, ModelRow>(
        "UPDATE models SET label=$1, provider=$2, model_name=$3, api_base=$4, api_key=$5, extra_body=$6
         WHERE id=$7 AND user_id=$8 AND scope='user'
         RETURNING *",
    )
    .bind(&req.label)
    .bind(&req.provider)
    .bind(&req.model_name)
    .bind(&req.api_base)
    .bind(&req.api_key)
    .bind(&req.extra_body)
    .bind(id)
    .bind(auth.user_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::not_found("模型不存在"))?;

    Ok(Json(ModelResponse::from(row)))
}

/// Delete a user-scoped model (must belong to caller).
pub async fn delete_model(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Value>> {
    let result = sqlx::query(
        "DELETE FROM models WHERE id=$1 AND user_id=$2 AND scope='user'",
    )
    .bind(id)
    .bind(auth.user_id)
    .execute(&state.pool)
    .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::not_found("模型不存在"));
    }

    Ok(Json(json!({ "ok": true })))
}

// ── Admin endpoints ───────────────────────────────────────────────────────────

fn guard_admin(auth: &AuthUser) -> AppResult<()> {
    if !auth.is_admin {
        return Err(AppError::forbidden("仅管理员可访问"));
    }
    Ok(())
}

/// Admin: list all global models.
pub async fn admin_list_models(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<Vec<ModelResponse>>> {
    guard_admin(&auth)?;
    let rows = sqlx::query_as::<_, ModelRow>(
        "SELECT * FROM models WHERE scope = 'global' ORDER BY created_at ASC",
    )
    .fetch_all(&state.pool)
    .await?;

    Ok(Json(rows.into_iter().map(ModelResponse::from).collect()))
}

/// Admin: create a global model.
pub async fn admin_create_model(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<UpsertModelRequest>,
) -> AppResult<Json<ModelResponse>> {
    guard_admin(&auth)?;
    let row = sqlx::query_as::<_, ModelRow>(
        "INSERT INTO models (user_id, scope, label, provider, model_name, api_base, api_key, extra_body)
         VALUES (NULL, 'global', $1, $2, $3, $4, $5, $6)
         RETURNING *",
    )
    .bind(&req.label)
    .bind(&req.provider)
    .bind(&req.model_name)
    .bind(&req.api_base)
    .bind(&req.api_key)
    .bind(&req.extra_body)
    .fetch_one(&state.pool)
    .await?;

    Ok(Json(ModelResponse::from(row)))
}

/// Admin: update a global model.
pub async fn admin_update_model(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<UpsertModelRequest>,
) -> AppResult<Json<ModelResponse>> {
    guard_admin(&auth)?;
    let row = sqlx::query_as::<_, ModelRow>(
        "UPDATE models SET label=$1, provider=$2, model_name=$3, api_base=$4, api_key=$5, extra_body=$6
         WHERE id=$7 AND scope='global'
         RETURNING *",
    )
    .bind(&req.label)
    .bind(&req.provider)
    .bind(&req.model_name)
    .bind(&req.api_base)
    .bind(&req.api_key)
    .bind(&req.extra_body)
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::not_found("模型不存在"))?;

    Ok(Json(ModelResponse::from(row)))
}

/// Admin: delete a global model.
pub async fn admin_delete_model(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Value>> {
    guard_admin(&auth)?;
    let result = sqlx::query("DELETE FROM models WHERE id=$1 AND scope='global'")
        .bind(id)
        .execute(&state.pool)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::not_found("模型不存在"));
    }

    Ok(Json(json!({ "ok": true })))
}

/// Admin: set a global model as default (clears any existing default first).
pub async fn admin_set_default_model(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Value>> {
    guard_admin(&auth)?;

    let mut tx = state.pool.begin().await?;

    sqlx::query("UPDATE models SET is_default = false WHERE scope = 'global'")
        .execute(&mut *tx)
        .await?;

    let rows = sqlx::query(
        "UPDATE models SET is_default = true WHERE id=$1 AND scope='global'",
    )
    .bind(id)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    if rows.rows_affected() == 0 {
        return Err(AppError::not_found("模型不存在"));
    }

    Ok(Json(json!({ "ok": true })))
}
