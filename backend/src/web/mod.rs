pub mod auth;
pub mod conversations;
pub mod files;
pub mod history;
pub mod sessions;
pub mod users;
pub mod ws;

use std::{path::Path, sync::Arc};

use axum::{
    Router,
    routing::{delete, get, patch, post},
};
use axum::extract::DefaultBodyLimit;
use sqlx::PgPool;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::{ServeDir, ServeFile};

use conversations::*;
use files::{download_file, preview_file, upload_file};
use history::*;
use sessions::*;
use users::*;
use ws::*;

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
        // ── File download / preview ───────────────────────────────────────────
        .route("/api/files", get(download_file))
        .route("/api/files", post(upload_file))
        .route("/api/files/preview", get(preview_file))
        // ── WebSocket ─────────────────────────────────────────────────────────
        .route("/ws/{id}", get(ws_handler))
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
