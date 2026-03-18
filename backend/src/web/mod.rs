pub mod admin;
pub mod auth;
pub mod conversations;
pub mod files;
pub mod history;
pub mod mcps;
pub mod sessions;
pub mod settings;
pub mod sse;
pub mod users;

use std::{path::Path, sync::Arc};

use admin::{
    create_app_skill, delete_app_skill, get_admin_config, list_app_skills, update_admin_config,
    update_app_skill, list_users, create_user, update_user, delete_user, reset_user_password,
    list_audit_logs,
    list_global_mcps, create_global_mcp, update_global_mcp, delete_global_mcp,
};
use axum::extract::DefaultBodyLimit;
use axum::{
    Router,
    routing::{delete, get, patch, post, put},
};
use mcps::{create_mcp, delete_mcp, list_mcps, update_mcp};
use settings::{
    create_skill, delete_skill, get_settings, list_skills, update_settings, update_skill,
};
use sqlx::PgPool;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::{ServeDir, ServeFile};

use conversations::*;
use files::{download_file, preview_file, upload_file, upload_avatar, get_avatar};
use history::*;
use sessions::*;
use sse::*;
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

pub fn create_router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let public_path = Path::new(&state.public_path);
    let artifacts_path = Path::new(&state.artifacts_path);

    Router::new()
        // ── Auth ──────────────────────────────────────────────────────────────
        .route("/api/sessions", post(login))
        .route("/api/sessions", delete(logout))
        // ── Users ─────────────────────────────────────────────────────────────
        .route("/api/users", post(register))
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
        .route("/api/admin/skills", get(list_app_skills))
        .route("/api/admin/skills", post(create_app_skill))
        .route("/api/admin/skills/{id}", put(update_app_skill))
        .route("/api/admin/skills/{id}", delete(delete_app_skill))
        // ── Admin User Management ────────────────────────────────────────────
        .route("/api/admin/users", get(list_users))
        .route("/api/admin/users", post(create_user))
        .route("/api/admin/users/{id}", put(update_user))
        .route("/api/admin/users/{id}", delete(delete_user))
        .route("/api/admin/users/{id}/reset-password", post(reset_user_password))
        // ── Admin Audit Logs ─────────────────────────────────────────────────
        .route("/api/admin/audit-logs", get(list_audit_logs))
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
        // ── MCPs ──────────────────────────────────────────────────────────────
        .route("/api/mcps", get(list_mcps))
        .route("/api/mcps", post(create_mcp))
        .route("/api/mcps/{id}", put(update_mcp))
        .route("/api/mcps/{id}", delete(delete_mcp))
        // ── File download / preview ───────────────────────────────────────────
        .route("/api/files", get(download_file))
        .route("/api/files", post(upload_file))
        .route("/api/files/preview", get(preview_file))
        // ── Chat (SSE streaming) ──────────────────────────────────────────────
        .route(
            "/api/conversations/{id}/messages",
            post(send_message_handler),
        )
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
