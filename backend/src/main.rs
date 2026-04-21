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
use tracing::{info, warn};
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

    bootstrap_admin_if_empty(&pool).await;

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

// If no users exist, check for INITIAL_ADMIN_USERNAME / INITIAL_ADMIN_PASSWORD env
// vars and create an admin. Otherwise log a prominent warning pointing the operator
// at the open registration window (first /api/auth/register call becomes admin).
async fn bootstrap_admin_if_empty(pool: &sqlx::PgPool) {
    let count: i64 = match sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(pool)
        .await
    {
        Ok(n) => n,
        Err(e) => {
            warn!(error = %e, "bootstrap: failed to count users");
            return;
        }
    };
    if count > 0 {
        return;
    }

    let username = std::env::var("INITIAL_ADMIN_USERNAME").ok();
    let password = std::env::var("INITIAL_ADMIN_PASSWORD").ok();

    match (username, password) {
        (Some(u), Some(p)) if !u.trim().is_empty() && !p.is_empty() => {
            let name = u.trim().to_string();
            if name.len() > 32
                || name
                    .chars()
                    .any(|c| !c.is_alphanumeric() && c != '_' && c != '-')
            {
                warn!("INITIAL_ADMIN_USERNAME invalid (need 1–32 chars, alnum/_/-) — skipping");
                return;
            }
            if p.len() < 8 {
                warn!("INITIAL_ADMIN_PASSWORD too short (need ≥8) — skipping");
                return;
            }
            let hash = match bcrypt::hash(&p, bcrypt::DEFAULT_COST) {
                Ok(h) => h,
                Err(e) => {
                    warn!(error = %e, "bootstrap: bcrypt failed");
                    return;
                }
            };
            let invite_code = crate::web::github_oauth::gen_invite_code();
            let res = sqlx::query(
                "INSERT INTO users (name, password_hash, display_name, invite_code, is_admin)
                 VALUES ($1, $2, $1, $3, true)",
            )
            .bind(&name)
            .bind(&hash)
            .bind(&invite_code)
            .execute(pool)
            .await;
            match res {
                Ok(_) => info!(user = %name, "bootstrap: admin created from INITIAL_ADMIN_* env"),
                Err(e) => warn!(error = %e, "bootstrap: admin insert failed"),
            }
        }
        _ => {
            warn!(
                "No users exist. The first /api/auth/register call will be granted admin. \
                 Register immediately at http://localhost:<PORT>/login — or set \
                 INITIAL_ADMIN_USERNAME and INITIAL_ADMIN_PASSWORD env vars and restart."
            );
        }
    }
}
