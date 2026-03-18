mod audit;
mod config;
mod db;
mod embedding;
mod errors;
mod sandbox;
mod spells;
mod state;
mod web;

use std::sync::Arc;

use sqlx::postgres::PgPoolOptions;
use tracing::info;
use tracing_subscriber::{EnvFilter, fmt};

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() {
    // env::set_current_dir(
    //     env::var("HOME")
    //         .or_else(|_| env::var("USERPROFILE"))
    //         .unwrap_or_else(|_| String::from("/root")),
    // )
    // .unwrap();

    fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let env_cfg = config::EnvConfig::load();

    info!("familiar starting");

    // ── Database ──────────────────────────────────────────────────────────────

    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(&env_cfg.database_url)
        .await
        .unwrap_or_else(|e| panic!("failed to connect to database: {e}"));

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .unwrap_or_else(|e| panic!("migration failed: {e}"));

    info!("database connected and migrations applied");

    let cfg = config::Config::load_from_db(&pool)
        .await
        .unwrap_or_else(|e| panic!("failed to load app configuration from DB: {e}"));

    // ── App state ─────────────────────────────────────────────────────────────

    let mcp_tools = state::AppState::init_mcp(&cfg.mcp).await;

    let state = Arc::new(state::AppState::new(&cfg, pool, mcp_tools));
    let web_state = web::AppState(Arc::clone(&state));

    // ── Web server ────────────────────────────────────────────────────────────

    let router = web::create_router(web_state);
    let addr = format!("0.0.0.0:{}", cfg.server.port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| panic!("failed to bind {addr}: {e}"));

    info!(addr = %addr, "listening");

    axum::serve(listener, router)
        .await
        .unwrap_or_else(|e| panic!("server error: {e}"));
}
