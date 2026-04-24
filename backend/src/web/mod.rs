pub mod admin;
pub mod auth;
pub mod conversations;
pub mod files;
pub mod github_oauth;
pub mod history;
pub mod invite_codes;
pub mod mcps;
pub mod models;
pub mod sessions;
pub mod settings;
pub mod sse;
pub mod tunnel;
pub mod users;

use std::{path::Path, sync::Arc};

use admin::{
    create_app_skill, create_catalog_entry, create_global_mcp, create_user, delete_app_skill,
    delete_catalog_entry, delete_global_mcp, delete_user, get_admin_config, get_token_usage,
    get_token_usage_by_user, get_token_usage_conversations, get_token_usage_daily, list_app_skills,
    list_audit_logs, list_catalog, list_global_mcps, list_users, reset_user_password, run_sql,
    update_admin_config, update_app_skill, update_catalog_entry, update_global_mcp, update_user,
};
use axum::extract::DefaultBodyLimit;
use axum::http::HeaderValue;
use axum::{
    Router,
    routing::{delete, get, patch, post, put},
};
use mcps::{create_mcp, delete_mcp, list_mcps, update_mcp};
use models::{
    admin_create_model, admin_delete_model, admin_list_model_permissions, admin_list_models,
    admin_update_model, admin_update_model_permissions, create_model, delete_model, list_models,
    update_model,
};
use settings::{
    create_skill, delete_skill, get_settings, list_skills, update_settings, update_skill,
};
use sqlx::PgPool;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::{ServeDir, ServeFile};

use conversations::*;
use files::{download_file, get_avatar, preview_file, upload_avatar, upload_file};
use history::*;
use sessions::*;
use sse::{
    activate_handler, branch_handler, reattach_handler, send_message_handler, sse_handler,
    stream_abort_handler, stream_answer_handler, stream_interrupt_handler,
};
use users::*;

/// Web-layer application state — cheaply cloneable.
#[derive(Clone)]
pub struct AppState(pub Arc<crate::state::AppState>);

impl std::ops::Deref for AppState {
    type Target = crate::state::AppState;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Allow `AuthUser` extractor to pull the pool out of `AppState`.
impl AsRef<PgPool> for AppState {
    fn as_ref(&self) -> &PgPool {
        &self.0.pool
    }
}

pub fn create_router(state: AppState, allowed_origin: Option<&str>) -> Router {
    let cors = if let Some(origin) = allowed_origin.and_then(|o| o.parse::<HeaderValue>().ok()) {
        CorsLayer::new()
            .allow_origin(origin)
            .allow_methods(Any)
            .allow_headers(Any)
    } else {
        CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any)
    };

    let public_path = Path::new(&state.public_path);
    let artifacts_path = Path::new(&state.artifacts_path);

    Router::new()
        // ── Auth ──────────────────────────────────────────────────────────────
        .route("/api/sessions", post(login))
        .route("/api/sessions", delete(logout))
        .route("/api/auth/github", get(github_oauth::github_login))
        .route(
            "/api/auth/github/callback",
            get(github_oauth::github_callback),
        )
        .route(
            "/api/auth/register",
            post(invite_codes::register_with_invite),
        )
        .route("/api/auth/status", get(invite_codes::auth_status))
        // ── Invite Codes (admin) ──────────────────────────────────────────────
        .route(
            "/api/admin/invite-codes",
            get(invite_codes::list_invite_codes),
        )
        .route(
            "/api/admin/invite-codes",
            post(invite_codes::create_invite_code),
        )
        .route(
            "/api/admin/invite-codes/{code}",
            delete(invite_codes::delete_invite_code),
        )
        // ── Users ─────────────────────────────────────────────────────────────
        .route("/api/users/me", get(get_me))
        .route("/api/users/me/profile", put(update_profile))
        .route("/api/users/me/password", put(update_password))
        .route("/api/users/me/avatar", post(upload_avatar))
        // ── Avatars ───────────────────────────────────────────────────────────
        .route("/api/avatars/{user_id}", get(get_avatar))
        // ── Settings ──────────────────────────────────────────────────────────
        .route("/api/settings", get(get_settings))
        .route("/api/settings", post(update_settings))
        // ── Admin Config ─────────────────────────────────────────────────────
        .route("/api/admin/config", get(get_admin_config))
        .route("/api/admin/config", post(update_admin_config))
        .route("/api/admin/mcps", get(list_global_mcps))
        .route("/api/admin/mcps", post(create_global_mcp))
        .route("/api/admin/mcps/{id}", put(update_global_mcp))
        .route("/api/admin/mcps/{id}", delete(delete_global_mcp))
        .route("/api/admin/catalog", get(list_catalog))
        .route("/api/admin/catalog", post(create_catalog_entry))
        .route("/api/admin/catalog/{id}", put(update_catalog_entry))
        .route("/api/admin/catalog/{id}", delete(delete_catalog_entry))
        .route("/api/admin/skills", get(list_app_skills))
        .route("/api/admin/skills", post(create_app_skill))
        .route("/api/admin/skills/{id}", put(update_app_skill))
        .route("/api/admin/skills/{id}", delete(delete_app_skill))
        // ── Admin User Management ────────────────────────────────────────────
        .route("/api/admin/users", get(list_users))
        .route("/api/admin/users", post(create_user))
        .route("/api/admin/users/{id}", put(update_user))
        .route("/api/admin/users/{id}", delete(delete_user))
        .route(
            "/api/admin/users/{id}/reset-password",
            post(reset_user_password),
        )
        // ── Admin Audit Logs ─────────────────────────────────────────────────
        .route("/api/admin/audit-logs", get(list_audit_logs))
        .route("/api/admin/token-usage", get(get_token_usage))
        .route(
            "/api/admin/token-usage/by-user",
            get(get_token_usage_by_user),
        )
        .route(
            "/api/admin/token-usage/conversations",
            get(get_token_usage_conversations),
        )
        .route("/api/admin/token-usage/daily", get(get_token_usage_daily))
        .route("/api/admin/sql", post(run_sql))
        // ── User Skills (per-user) ─────────────────────────────────────────────
        .route("/api/skills", get(list_skills))
        .route("/api/skills", post(create_skill))
        .route("/api/skills/{id}", put(update_skill))
        .route("/api/skills/{id}", delete(delete_skill))
        // ── Conversations ─────────────────────────────────────────────────────
        .route("/api/conversations", get(list_conversations))
        .route("/api/conversations", post(create_conversation))
        .route("/api/conversations/{id}", delete(delete_conversation))
        .route("/api/conversations/{id}", patch(rename_conversation))
        .route("/api/conversations/{id}/title", post(auto_title))
        // ── Messages ──────────────────────────────────────────────────────────
        .route("/api/conversations/{id}/messages", get(list_messages))
        .route("/api/search", get(search_messages))
        // ── MCPs ──────────────────────────────────────────────────────────────
        .route("/api/mcps", get(list_mcps))
        .route("/api/mcps", post(create_mcp))
        .route("/api/mcps/{id}", put(update_mcp))
        .route("/api/mcps/{id}", delete(delete_mcp))
        // ── Models ────────────────────────────────────────────────────────────
        .route("/api/models", get(list_models))
        .route("/api/models", post(create_model))
        .route("/api/models/{id}", put(update_model))
        .route("/api/models/{id}", delete(delete_model))
        .route("/api/admin/models", get(admin_list_models))
        .route("/api/admin/models", post(admin_create_model))
        .route("/api/admin/models/{id}", put(admin_update_model))
        .route("/api/admin/models/{id}", delete(admin_delete_model))
        .route(
            "/api/admin/model-permissions",
            get(admin_list_model_permissions),
        )
        .route(
            "/api/admin/model-permissions",
            put(admin_update_model_permissions),
        )
        // ── File download / preview ───────────────────────────────────────────
        .route("/api/files", get(download_file))
        .route("/api/files", post(upload_file))
        .route("/api/files/preview", get(preview_file))
        // ── 客户端隧道 (WebSocket) ────────────────────────────────────────────
        .route("/api/tunnel", get(tunnel::tunnel_handler))
        // ── Chat (SSE streaming) ──────────────────────────────────────────────
        .route(
            "/api/conversations/{id}/messages",
            post(send_message_handler),
        )
        .route("/api/conversations/{id}/reattach", post(reattach_handler))
        .route("/api/conversations/{id}/branch", post(branch_handler))
        .route("/api/conversations/{id}/activate", post(activate_handler))
        .route("/api/stream/{stream_id}", get(sse_handler))
        .route("/api/stream/{stream_id}/abort", post(stream_abort_handler))
        .route(
            "/api/stream/{stream_id}/interrupt",
            post(stream_interrupt_handler),
        )
        .route(
            "/api/stream/{stream_id}/answer",
            post(stream_answer_handler),
        )
        // model artifacts
        .nest_service("/artifacts", ServeDir::new(artifacts_path))
        // ── Static frontend ───────────────────────────────────────────────────
        .fallback_service(
            ServeDir::new(public_path)
                .not_found_service(ServeFile::new(public_path.join("index.html"))),
        )
        .layer(cors)
        .layer(DefaultBodyLimit::max(50 * 1024 * 1024))
        .with_state(state)
}
