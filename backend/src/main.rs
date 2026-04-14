mod audit;
mod compact;
mod config;
mod db;
mod embedding;
mod errors;
mod prompt;
mod prompt_template;
mod sandbox;
mod spells;
mod state;
mod web;
mod worker;

use std::sync::Arc;

use sqlx::postgres::PgPoolOptions;
use tracing::info;
use tracing_subscriber::{EnvFilter, fmt};

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() {
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

    // ── App state (stateless — no in-memory caches) ──────────────────────────

    let state = Arc::new(state::AppState::new(&cfg, pool.clone()));
    let web_state = web::AppState(Arc::clone(&state));

    // ── Background: seal orphaned streaming messages ──────────────────────────
    // If a worker panics, its streaming=true message row is never sealed.
    // This task finds such rows every 60 s and seals them.
    {
        let cleanup_pool = pool.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
            loop {
                interval.tick().await;
                let _ = sqlx::query(
                    "UPDATE messages SET streaming = false \
                     WHERE streaming = true \
                       AND job_id IN (\
                           SELECT id FROM generation_jobs \
                           WHERE status NOT IN ('pending', 'running')\
                       )",
                )
                .execute(&cleanup_pool)
                .await;
            }
        });
    }

    // ── Web server ────────────────────────────────────────────────────────────

    let allowed_origin = std::env::var("ALLOWED_ORIGIN").ok();
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3000);
    let router = web::create_router(web_state, allowed_origin.as_deref());
    let addr = format!("0.0.0.0:{port}");
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| panic!("failed to bind {addr}: {e}"));

    info!(addr = %addr, "listening");

    axum::serve(listener, router)
        .await
        .unwrap_or_else(|e| panic!("server error: {e}"));
}
