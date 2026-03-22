mod a2a_spell;
mod history_spell;
mod manage_mcp_spell;
mod plan_spell;
mod skill_spell;
mod sourcegraph_spell;
mod spawn_spell;
mod ui_spells;

use agentix::ToolCommand;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use agentix::McpTool;
pub use agentix::tool_trait::ToolBundle;
use uuid::Uuid;

use crate::config::{McpCatalogEntry, ModelConfig};
use crate::db::Db;
use crate::embedding::EmbeddingClient;
#[allow(unused)]
use a2a_spell::A2aSpell;
use history_spell::HistorySpell;
use manage_mcp_spell::ManageMcpSpell;
use plan_spell::PlanSpell;
use skill_spell::SkillSpell;
use sourcegraph_spell::search_code;
use spawn_spell::SpawnSpell;
use ui_spells::UiSpells;

// ── Spell factory ─────────────────────────────────────────────────────────────

/// All runtime dependencies required to build the full built-in spell bundle.
/// Pass to `build_all_spells` in `build_agent`; no spell type needs to be
/// imported outside this module.
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
    // ManageMcpSpell
    pub tool_inject_tx: tokio::sync::mpsc::UnboundedSender<ToolCommand>,
    pub pool: sqlx::PgPool,
    pub user_id: Uuid,
    pub sandbox: Arc<crate::sandbox::SandboxManager>,
    pub mcp_catalog: Vec<McpCatalogEntry>,
    // Shared
    pub abort_flag: Arc<AtomicBool>,
}

/// Build the complete built-in spell bundle from the given dependencies.
/// Returns a `ToolBundle` ready to be passed to `builder.add_tool(...)`.
pub fn build_all_spells(deps: SpellDeps) -> ToolBundle {
    ToolBundle::new()
        + SkillSpell {
            pool: deps.pool.clone(),
            user_id: deps.user_id,
        }
        // + A2aSpell
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
            embed: deps.embed,
            conversation_id: deps.conversation_id,
        }
        + ManageMcpSpell {
            mcp_tools: deps.mcp_tools,
            tool_inject_tx: deps.tool_inject_tx,
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
