mod config;
mod db;
mod embedding;
mod errors;
mod spells;
mod state;
mod web;

use std::sync::Arc;

use sqlx::postgres::PgPoolOptions;
use tracing::info;
use tracing_subscriber::{EnvFilter, fmt};

#[tokio::main]
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

    let cfg = config::Config::load();

    info!("familiar starting");

    // ── Database ──────────────────────────────────────────────────────────────

    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(&cfg.secrets.database_url)
        .await
        .unwrap_or_else(|e| panic!("failed to connect to database: {e}"));

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .unwrap_or_else(|e| panic!("migration failed: {e}"));

    info!("database connected and migrations applied");

    // ── App state ─────────────────────────────────────────────────────────────

    let mcp_tools = state::AppState::init_mcp(&cfg.mcp).await;

    // Count tool definitions from built-in spells (no MCP).
    // We build a throw-away agent to measure this rather than hard-coding a number.
    let builtin_tool_count = {
        use ds_api::Tool;
        use spells::{
            A2aSpell, AskUserSpell, CommandSpell, FileSpell, HistorySpell, ManageMcpSpell,
            OutlineSpell, PresentFileSpell, ScriptSpell, SearchSpell,
        };
        use std::sync::Arc;
        use std::sync::atomic::AtomicBool;

        // Placeholder values — we only need raw_tools().len(), not real operation.
        let dummy_pending = Arc::new(tokio::sync::Mutex::new(None));
        let dummy_stale = Arc::new(AtomicBool::new(false));
        let dummy_mcp: Arc<tokio::sync::Mutex<Vec<(String, ds_api::McpTool)>>> =
            Arc::new(tokio::sync::Mutex::new(vec![]));
        let dummy_db = db::Db::new(pool.clone());
        let dummy_embed = embedding::EmbeddingClient::new(
            cfg.embedding.api_key.clone(),
            cfg.embedding.api_base.clone(),
            cfg.embedding.name.clone(),
        );
        let dummy_conv = uuid::Uuid::nil();

        CommandSpell.raw_tools().len()
            + FileSpell.raw_tools().len()
            + ScriptSpell.raw_tools().len()
            + PresentFileSpell.raw_tools().len()
            + A2aSpell.raw_tools().len()
            + SearchSpell.raw_tools().len()
            + OutlineSpell.raw_tools().len()
            + AskUserSpell {
                pending: dummy_pending,
            }
            .raw_tools()
            .len()
            + ManageMcpSpell {
                mcp_tools: dummy_mcp,
                agent_stale: dummy_stale,
                builtin_tool_count: 0,
                max_tools: 0,
            }
            .raw_tools()
            .len()
            + HistorySpell {
                db: dummy_db,
                embed: dummy_embed,
                conversation_id: dummy_conv,
            }
            .raw_tools()
            .len()
    };
    info!("built-in tool count: {builtin_tool_count}");

    let state = Arc::new(state::AppState::new(
        &cfg,
        pool,
        mcp_tools,
        builtin_tool_count,
    ));
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
