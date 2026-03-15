pub mod auth;
pub mod conversations;
pub mod files;
pub mod history;
pub mod mcps;
pub mod sessions;
pub mod sse;
pub mod users;

use std::{path::Path, sync::Arc};

use axum::extract::DefaultBodyLimit;
use axum::{
    Router,
    routing::{delete, get, patch, post, put},
};
use mcps::{create_mcp, delete_mcp, list_mcps, update_mcp};
use sqlx::PgPool;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::{ServeDir, ServeFile};

use conversations::*;
use files::{download_file, preview_file, upload_file};
use history::*;
use sessions::*;
use sse::*;
use users::*;

use crate::config::Config;

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

    let config = Config::load();
    let public_path = Path::new(&config.public_path);
    let artifacts_path = Path::new(&config.artifacts_path);

    Router::new()
        // ── Auth ──────────────────────────────────────────────────────────────
        .route("/api/sessions", post(login))
        .route("/api/sessions", delete(logout))
        // ── Users ─────────────────────────────────────────────────────────────
        .route("/api/users", post(register))
        .route("/api/users/me", get(get_me))
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
