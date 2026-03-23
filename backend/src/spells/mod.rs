mod manage_mcp_spell;
mod spawn_spell;
mod ui_spells;

use shared_backend::spells::history_spell::HistorySpell;
use shared_backend::spells::plan_spell::PlanSpell;
use shared_backend::spells::skill_spell::SkillSpell;
use shared_backend::spells::sourcegraph_spell::search_code;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use agentix::{Agent, McpTool};
pub use agentix::tool_trait::ToolBundle;
use tokio::sync::OnceCell;
use uuid::Uuid;

use crate::config::{McpCatalogEntry, ModelConfig};
use crate::db::Db;
use crate::embedding::EmbeddingClient;
use manage_mcp_spell::ManageMcpSpell;
use spawn_spell::SpawnSpell;
use ui_spells::UiSpells;

// ── Spell factory ─────────────────────────────────────────────────────────────

/// All runtime dependencies required to build the full built-in spell bundle.
pub struct SpellDeps {
    // UiSpells
    pub ask_pending: Arc<tokio::sync::Mutex<Option<tokio::sync::oneshot::Sender<String>>>>,
    pub subagent_prompt: Option<String>,
    // SpawnSpell
    pub cheap_model: ModelConfig,
    pub mcp_tools: Arc<tokio::sync::Mutex<Vec<(String, McpTool)>>>,
    pub spawn_tx: tokio::sync::broadcast::Sender<String>,
    // HistorySpell
    pub db: Db,
    pub embed: EmbeddingClient,
    pub conversation_id: Uuid,
    // ManageMcpSpell — filled in after the Agent Arc is created
    pub agent: Arc<OnceCell<Arc<tokio::sync::Mutex<Agent>>>>,
    pub pool: sqlx::PgPool,
    pub user_id: Uuid,
    pub sandbox: Arc<crate::sandbox::SandboxManager>,
    pub mcp_catalog: Vec<McpCatalogEntry>,
    // Shared
    pub abort_flag: Arc<AtomicBool>,
}

/// Build the complete built-in spell bundle from the given dependencies.
pub fn build_all_spells(deps: SpellDeps) -> ToolBundle {
    ToolBundle::new()
        + SkillSpell {
            pool: deps.pool.clone(),
            user_id: deps.user_id,
        }
        + UiSpells {
            ask_pending: deps.ask_pending,
            user_id: deps.user_id,
            conversation_id: deps.conversation_id,
            sandbox: deps.sandbox.clone(),
        }
        + search_code
        + SpawnSpell {
            cheap_model: deps.cheap_model,
            subagent_prompt: deps.subagent_prompt,
            mcp_tools: Arc::clone(&deps.mcp_tools),
            broadcast_tx: deps.spawn_tx,
            abort_flag: Arc::clone(&deps.abort_flag),
        }
        + HistorySpell {
            db: deps.db,
            embedding: deps.embed,
            conversation_id: deps.conversation_id,
        }
        + ManageMcpSpell {
            mcp_tools: deps.mcp_tools,
            agent: deps.agent,
            pool: deps.pool.clone(),
            user_id: deps.user_id,
            conversation_id: deps.conversation_id,
            sandbox: deps.sandbox,
            catalog: deps.mcp_catalog,
        }
        + PlanSpell {
            pool: deps.pool,
            conversation_id: deps.conversation_id,
        }
}
