use axum::{
    Json,
    extract::{Path, State},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

use crate::audit::log_audit;
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
    pub kind: String,
    pub initial_available: bool,
    pub created_at: sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>,
    pub compact_trigger_tokens: i64,
    pub compact_tail_tokens: i64,
    pub reasoning_effort: Option<String>,
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
    pub initial_available: bool,
    pub kind: String,
    pub created_at: String,
    pub compact_trigger_tokens: i64,
    pub compact_tail_tokens: i64,
    /// Null = leave provider default; one of
    /// `'none' | 'minimal' | 'low' | 'medium' | 'high' | 'xhigh' | 'max'`.
    pub reasoning_effort: Option<String>,
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
            initial_available: r.initial_available,
            kind: r.kind,
            created_at: r.created_at.to_rfc3339(),
            compact_trigger_tokens: r.compact_trigger_tokens,
            compact_tail_tokens: r.compact_tail_tokens,
            reasoning_effort: r.reasoning_effort,
        }
    }
}

const VALID_REASONING_EFFORTS: &[&str] =
    &["none", "minimal", "low", "medium", "high", "xhigh", "max"];

/// Normalize incoming effort strings: treat empty/whitespace as null, and
/// reject anything outside the known enum so a typo fails fast.
fn normalize_reasoning_effort(raw: &Option<String>) -> AppResult<Option<String>> {
    match raw {
        None => Ok(None),
        Some(s) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                return Ok(None);
            }
            if !VALID_REASONING_EFFORTS.contains(&trimmed) {
                return Err(AppError::bad_request(
                    "reasoning_effort 非法（仅允许 none/minimal/low/medium/high/xhigh/max 或留空）",
                ));
            }
            Ok(Some(trimmed.to_string()))
        }
    }
}

fn default_kind() -> String {
    "api".to_string()
}

/// Used by user-scoped and admin-scoped upsert endpoints.
/// Admin-only fields (role/initial_available/is_default) are accepted on the
/// admin PUT but ignored by the user PUT.
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
    /// Optional reasoning-effort hint. `None`/empty keeps provider default.
    #[serde(default)]
    pub reasoning_effort: Option<String>,
    // Admin-only knobs (optional; admin PUT applies them, user PUT ignores).
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub initial_available: Option<bool>,
    #[serde(default)]
    pub is_default: Option<bool>,
}

// ── User endpoints ────────────────────────────────────────────────────────────

/// List models visible to the current user: global + their own.
pub async fn list_models(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<Vec<ModelResponse>>> {
    let rows = sqlx::query_as::<_, ModelRow>(
        "SELECT * FROM models
         WHERE (
             scope = 'global'
             AND COALESCE(
                 (
                     SELECT allowed
                     FROM user_model_permissions ump
                     WHERE ump.user_id = $1 AND ump.model_id = models.id
                 ),
                 initial_available
             )
         )
         OR (scope = 'user' AND user_id = $1)
         ORDER BY scope DESC, created_at ASC",
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
    let effort = normalize_reasoning_effort(&req.reasoning_effort)?;
    let row = sqlx::query_as::<_, ModelRow>(
        "INSERT INTO models (user_id, scope, label, provider, model_name, api_base, api_key, extra_body, kind,
                             compact_trigger_tokens, compact_tail_tokens, reasoning_effort)
         VALUES ($1, 'user', $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
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
    .bind(&effort)
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
    let effort = normalize_reasoning_effort(&req.reasoning_effort)?;
    let row = sqlx::query_as::<_, ModelRow>(
        "UPDATE models SET label=$1, provider=$2, model_name=$3, api_base=$4,
         api_key = CASE WHEN $5 = '' THEN api_key ELSE $5 END,
         extra_body=$6, kind=$7,
         compact_trigger_tokens=$8, compact_tail_tokens=$9,
         reasoning_effort=$10
         WHERE id=$11 AND user_id=$12 AND scope='user'
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
    .bind(&effort)
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
/// Admin-only knobs (role/initial_available/is_default) fall back to column
/// defaults (NULL role, initial_available=true, is_default=false)
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

    let effort = normalize_reasoning_effort(&req.reasoning_effort)?;
    let row = sqlx::query_as::<_, ModelRow>(
        "INSERT INTO models
           (user_id, scope, label, provider, model_name, api_base, api_key,
            extra_body, kind, role, initial_available, is_default,
            compact_trigger_tokens, compact_tail_tokens, reasoning_effort)
         VALUES (NULL, 'global', $1, $2, $3, $4, $5, $6, $7,
                 $8, COALESCE($9, true), COALESCE($10, false), $11, $12, $13)
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
    .bind(req.initial_available)
    .bind(req.is_default)
    .bind(req.compact_trigger_tokens)
    .bind(req.compact_tail_tokens)
    .bind(&effort)
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(Json(ModelResponse::from(row)))
}

/// Admin: update a global model.
///
/// Applies every field the admin form sends in a single transaction:
/// base fields (label/provider/…/kind), role, visibility, default flag, and
/// initial_available. If is_default=true is set here, all other global
/// defaults are cleared atomically. Same single-winner semantics apply to role.
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
    let effort = normalize_reasoning_effort(&req.reasoning_effort)?;
    let row = sqlx::query_as::<_, ModelRow>(
        "UPDATE models SET label=$1, provider=$2, model_name=$3, api_base=$4,
         api_key = CASE WHEN $5 = '' THEN api_key ELSE $5 END,
         extra_body=$6, kind=$7,
         role       = $8,
         initial_available = COALESCE($9, initial_available),
         is_default = COALESCE($10, is_default),
         compact_trigger_tokens = $11,
         compact_tail_tokens    = $12,
         reasoning_effort       = $13
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
    .bind(req.initial_available)
    .bind(req.is_default)
    .bind(req.compact_trigger_tokens)
    .bind(req.compact_tail_tokens)
    .bind(&effort)
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

// ── Admin: model permission matrix ───────────────────────────────────────────

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct AdminModelPermissionUser {
    pub id: Uuid,
    pub name: String,
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub is_admin: bool,
    pub is_banned: bool,
}

#[derive(Debug, Serialize)]
pub struct AdminModelPermissionCell {
    pub user_id: Uuid,
    pub model_id: Uuid,
    /// The stored override. `null` means the user inherits the global default.
    pub override_allowed: Option<bool>,
    /// What the model picker will actually show for this user.
    pub effective_allowed: bool,
    pub inherited: bool,
    /// Reserved for hard blockers. Currently model access is fully controlled
    /// by `initial_available` plus per-user overrides.
    pub blocked_reason: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AdminModelPermissionsResponse {
    pub users: Vec<AdminModelPermissionUser>,
    pub models: Vec<ModelResponse>,
    pub permissions: Vec<AdminModelPermissionCell>,
}

#[derive(Debug, sqlx::FromRow)]
struct PermissionOverrideRow {
    user_id: Uuid,
    model_id: Uuid,
    allowed: bool,
}

pub async fn admin_list_model_permissions(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<AdminModelPermissionsResponse>> {
    guard_admin(&auth)?;

    let users = sqlx::query_as::<_, AdminModelPermissionUser>(
        "SELECT id, name, email, display_name, is_admin, COALESCE(is_banned, false) AS is_banned
         FROM users
         ORDER BY created_at DESC",
    )
    .fetch_all(&state.pool)
    .await?;

    let model_rows = sqlx::query_as::<_, ModelRow>(
        "SELECT * FROM models WHERE scope = 'global' ORDER BY created_at ASC",
    )
    .fetch_all(&state.pool)
    .await?;

    let overrides = sqlx::query_as::<_, PermissionOverrideRow>(
        "SELECT user_id, model_id, allowed FROM user_model_permissions",
    )
    .fetch_all(&state.pool)
    .await?;

    let override_map: HashMap<(Uuid, Uuid), bool> = overrides
        .into_iter()
        .map(|row| ((row.user_id, row.model_id), row.allowed))
        .collect();

    let mut permissions = Vec::with_capacity(users.len() * model_rows.len());
    for user in &users {
        for model in &model_rows {
            let override_allowed = override_map.get(&(user.id, model.id)).copied();
            let effective_allowed = override_allowed.unwrap_or(model.initial_available);

            permissions.push(AdminModelPermissionCell {
                user_id: user.id,
                model_id: model.id,
                override_allowed,
                effective_allowed,
                inherited: override_allowed.is_none(),
                blocked_reason: None,
            });
        }
    }

    Ok(Json(AdminModelPermissionsResponse {
        users,
        models: model_rows.into_iter().map(ModelResponse::from).collect(),
        permissions,
    }))
}

#[derive(Debug, Deserialize)]
pub struct ModelPermissionChange {
    pub user_id: Uuid,
    pub model_id: Uuid,
    /// `null` removes the override and returns the cell to inherited behavior.
    pub allowed: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateModelPermissionsRequest {
    pub changes: Vec<ModelPermissionChange>,
}

pub async fn admin_update_model_permissions(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<UpdateModelPermissionsRequest>,
) -> AppResult<Json<Value>> {
    guard_admin(&auth)?;

    if req.changes.len() > 2_000 {
        return Err(AppError::bad_request("一次最多更新 2000 个权限单元"));
    }

    let user_ids: HashSet<Uuid> = req.changes.iter().map(|c| c.user_id).collect();
    let model_ids: HashSet<Uuid> = req.changes.iter().map(|c| c.model_id).collect();

    if !user_ids.is_empty() {
        let existing_users: Vec<Uuid> =
            sqlx::query_scalar("SELECT id FROM users WHERE id = ANY($1)")
                .bind(user_ids.iter().copied().collect::<Vec<_>>())
                .fetch_all(&state.pool)
                .await?;
        if existing_users.len() != user_ids.len() {
            return Err(AppError::bad_request("包含不存在的用户"));
        }
    }

    if !model_ids.is_empty() {
        let existing_models: Vec<Uuid> =
            sqlx::query_scalar("SELECT id FROM models WHERE scope = 'global' AND id = ANY($1)")
                .bind(model_ids.iter().copied().collect::<Vec<_>>())
                .fetch_all(&state.pool)
                .await?;
        if existing_models.len() != model_ids.len() {
            return Err(AppError::bad_request("包含不存在的全局模型"));
        }
    }

    let mut tx = state.pool.begin().await?;
    for change in &req.changes {
        if let Some(allowed) = change.allowed {
            sqlx::query(
                "INSERT INTO user_model_permissions (user_id, model_id, allowed, updated_by)
                 VALUES ($1, $2, $3, $4)
                 ON CONFLICT (user_id, model_id)
                 DO UPDATE SET
                    allowed = EXCLUDED.allowed,
                    updated_by = EXCLUDED.updated_by,
                    updated_at = NOW()",
            )
            .bind(change.user_id)
            .bind(change.model_id)
            .bind(allowed)
            .bind(auth.user_id)
            .execute(&mut *tx)
            .await?;
        } else {
            sqlx::query("DELETE FROM user_model_permissions WHERE user_id = $1 AND model_id = $2")
                .bind(change.user_id)
                .bind(change.model_id)
                .execute(&mut *tx)
                .await?;
        }
    }
    tx.commit().await?;

    log_audit(
        &state.pool,
        Some(auth.user_id),
        None,
        "update_model_permissions",
        Some(json!({ "changes": req.changes.len() })),
        None,
    )
    .await?;

    Ok(Json(json!({ "ok": true })))
}
