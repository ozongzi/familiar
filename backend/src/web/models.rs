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
    pub role: Option<String>,
    pub visible: bool,
    pub kind: String,
    pub admin_only: bool,
    pub created_at: sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>,
    pub compact_trigger_tokens: i64,
    pub compact_tail_tokens: i64,
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
    pub role: Option<String>,
    pub visible: bool,
    pub kind: String,
    pub admin_only: bool,
    pub created_at: String,
    pub compact_trigger_tokens: i64,
    pub compact_tail_tokens: i64,
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
            role: r.role,
            visible: r.visible,
            kind: r.kind,
            admin_only: r.admin_only,
            created_at: r.created_at.to_rfc3339(),
            compact_trigger_tokens: r.compact_trigger_tokens,
            compact_tail_tokens: r.compact_tail_tokens,
        }
    }
}

fn default_kind() -> String {
    "api".to_string()
}

/// Used by user-scoped and admin-scoped upsert endpoints.
/// Admin-only fields (role/visible/is_default/admin_only) are accepted on the
/// admin PUT but ignored by the user PUT — keeping a single request struct
/// keeps the frontend types flat.
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
    #[serde(default = "default_kind")]
    pub kind: String,
    pub compact_trigger_tokens: i64,
    pub compact_tail_tokens: i64,
    // Admin-only knobs (optional; admin PUT applies them, user PUT ignores).
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub visible: Option<bool>,
    #[serde(default)]
    pub is_default: Option<bool>,
    #[serde(default)]
    pub admin_only: Option<bool>,
}

// ── User endpoints ────────────────────────────────────────────────────────────

/// List models visible to the current user: global + their own.
/// admin_only models are filtered out for non-admins (ToS compliance for
/// provider credentials that can only be shared with the instance operator).
pub async fn list_models(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<Vec<ModelResponse>>> {
    let rows = sqlx::query_as::<_, ModelRow>(
        "SELECT * FROM models
         WHERE (scope = 'global' OR user_id = $1)
           AND visible = true
           AND (NOT admin_only OR $2)
         ORDER BY scope DESC, created_at ASC",
    )
    .bind(auth.user_id)
    .bind(auth.is_admin)
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
        "INSERT INTO models (user_id, scope, label, provider, model_name, api_base, api_key, extra_body, kind,
                             compact_trigger_tokens, compact_tail_tokens)
         VALUES ($1, 'user', $2, $3, $4, $5, $6, $7, $8, $9, $10)
         RETURNING *",
    )
    .bind(auth.user_id)
    .bind(&req.label)
    .bind(&req.provider)
    .bind(&req.model_name)
    .bind(&req.api_base)
    .bind(&req.api_key)
    .bind(&req.extra_body)
    .bind(&req.kind)
    .bind(req.compact_trigger_tokens)
    .bind(req.compact_tail_tokens)
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
        "UPDATE models SET label=$1, provider=$2, model_name=$3, api_base=$4,
         api_key = CASE WHEN $5 = '' THEN api_key ELSE $5 END,
         extra_body=$6, kind=$7,
         compact_trigger_tokens=$8, compact_tail_tokens=$9
         WHERE id=$10 AND user_id=$11 AND scope='user'
         RETURNING *",
    )
    .bind(&req.label)
    .bind(&req.provider)
    .bind(&req.model_name)
    .bind(&req.api_base)
    .bind(&req.api_key)
    .bind(&req.extra_body)
    .bind(&req.kind)
    .bind(req.compact_trigger_tokens)
    .bind(req.compact_tail_tokens)
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
    let result = sqlx::query("DELETE FROM models WHERE id=$1 AND user_id=$2 AND scope='user'")
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
///
/// Admin-only knobs (role/visible/is_default/admin_only) fall back to column
/// defaults (NULL role, visible=true, is_default=false, admin_only=false)
/// when omitted — matching the legacy two-step create-then-set flow.
pub async fn admin_create_model(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<UpsertModelRequest>,
) -> AppResult<Json<ModelResponse>> {
    guard_admin(&auth)?;

    let mut tx = state.pool.begin().await?;

    if req.is_default == Some(true) {
        sqlx::query("UPDATE models SET is_default = false WHERE scope = 'global'")
            .execute(&mut *tx)
            .await?;
    }
    if let Some(ref role) = req.role {
        sqlx::query("UPDATE models SET role = NULL WHERE scope = 'global' AND role = $1")
            .bind(role)
            .execute(&mut *tx)
            .await?;
    }

    let row = sqlx::query_as::<_, ModelRow>(
        "INSERT INTO models
           (user_id, scope, label, provider, model_name, api_base, api_key,
            extra_body, kind, role, visible, is_default, admin_only,
            compact_trigger_tokens, compact_tail_tokens)
         VALUES (NULL, 'global', $1, $2, $3, $4, $5, $6, $7,
                 $8, COALESCE($9, true), COALESCE($10, false), COALESCE($11, false),
                 $12, $13)
         RETURNING *",
    )
    .bind(&req.label)
    .bind(&req.provider)
    .bind(&req.model_name)
    .bind(&req.api_base)
    .bind(&req.api_key)
    .bind(&req.extra_body)
    .bind(&req.kind)
    .bind(&req.role)
    .bind(req.visible)
    .bind(req.is_default)
    .bind(req.admin_only)
    .bind(req.compact_trigger_tokens)
    .bind(req.compact_tail_tokens)
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(Json(ModelResponse::from(row)))
}

/// Admin: update a global model.
///
/// Applies every field the admin form sends in a single transaction:
/// base fields (label/provider/…/kind), role, visibility, default flag, and
/// admin_only. If is_default=true is set here, all other global defaults are
/// cleared atomically. Same single-winner semantics apply to role.
pub async fn admin_update_model(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<UpsertModelRequest>,
) -> AppResult<Json<ModelResponse>> {
    guard_admin(&auth)?;

    let mut tx = state.pool.begin().await?;

    // Single-winner maintenance before we write the target row.
    if req.is_default == Some(true) {
        sqlx::query("UPDATE models SET is_default = false WHERE scope = 'global'")
            .execute(&mut *tx)
            .await?;
    }
    if let Some(ref role) = req.role {
        sqlx::query("UPDATE models SET role = NULL WHERE scope = 'global' AND role = $1")
            .bind(role)
            .execute(&mut *tx)
            .await?;
    }

    // The admin form is always a full-form submit, so we write the four
    // admin-scoped fields directly. COALESCE wouldn't work for role: Option's
    // None ↔ JSON null conflation means sending `role: null` (to clear a
    // role) would read as "field omitted" and keep the old value.
    let row = sqlx::query_as::<_, ModelRow>(
        "UPDATE models SET label=$1, provider=$2, model_name=$3, api_base=$4,
         api_key = CASE WHEN $5 = '' THEN api_key ELSE $5 END,
         extra_body=$6, kind=$7,
         role       = $8,
         visible    = COALESCE($9,  visible),
         is_default = COALESCE($10, is_default),
         admin_only = COALESCE($11, admin_only),
         compact_trigger_tokens = $12,
         compact_tail_tokens    = $13
         WHERE id=$14 AND scope='global'
         RETURNING *",
    )
    .bind(&req.label)
    .bind(&req.provider)
    .bind(&req.model_name)
    .bind(&req.api_base)
    .bind(&req.api_key)
    .bind(&req.extra_body)
    .bind(&req.kind)
    .bind(&req.role)
    .bind(req.visible)
    .bind(req.is_default)
    .bind(req.admin_only)
    .bind(req.compact_trigger_tokens)
    .bind(req.compact_tail_tokens)
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| AppError::not_found("模型不存在"))?;

    tx.commit().await?;

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
