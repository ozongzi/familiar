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
    let builtin_tool_count = {
        use ds_api::{Tool, ToolBundle};
        use spells::{FileSpells, HistorySpell, SearchSpells, ShellSpells, SpawnSpell, UiSpells};
        use std::sync::Arc;

        let dummy_pending = Arc::new(tokio::sync::Mutex::new(None));
        let dummy_mcp: Arc<tokio::sync::Mutex<Vec<(String, ds_api::McpTool)>>> =
            Arc::new(tokio::sync::Mutex::new(vec![]));
        let dummy_db = db::Db::new(pool.clone());
        let dummy_embed = embedding::EmbeddingClient::new(
            cfg.embedding.api_key.clone(),
            cfg.embedding.api_base.clone(),
            cfg.embedding.name.clone(),
        );
        let dummy_conv = uuid::Uuid::nil();
        let (dummy_tx, _) = tokio::sync::broadcast::channel::<String>(1);

        ToolBundle::new()
            .add(FileSpells)
            .add(ShellSpells)
            .add(SearchSpells)
            .raw_tools()
            .len()
            + UiSpells {
                ask_pending: dummy_pending,
            }
            .raw_tools()
            .len()
            + SpawnSpell {
                api_key: String::new(),
                api_base: String::new(),
                model_name: String::new(),
                mcp_tools: dummy_mcp,
                default_tools: vec![],
                broadcast_tx: dummy_tx,
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
