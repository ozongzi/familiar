mod a2a_spell;
mod history_spell;
mod manage_mcp_spell;
mod spawn_spell;
mod skill_spell;
mod ui_spells;

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use ds_api::ToolInjection;

pub use ds_api::tool_trait::ToolBundle;
use ds_api::McpTool;
use serde_json::Value;
use uuid::Uuid;

use a2a_spell::A2aSpell;
use history_spell::HistorySpell;
use manage_mcp_spell::ManageMcpSpell;
use spawn_spell::SpawnSpell;
use skill_spell::SkillSpell;
use ui_spells::UiSpells;
use crate::db::Db;
use crate::embedding::EmbeddingClient;


// ── Spell factory ─────────────────────────────────────────────────────────────

/// All runtime dependencies required to build the full built-in spell bundle.
/// Pass to `build_all_spells` in `build_agent`; no spell type needs to be
/// imported outside this module.
pub struct SpellDeps {
    // UiSpells
    pub ask_pending: Arc<tokio::sync::Mutex<Option<tokio::sync::oneshot::Sender<String>>>>,
    pub subagent_prompt: Option<String>,
    // SpawnSpell
    pub api_key: String,
    pub api_base: String,
    pub model_name: String,
    pub extra_body: HashMap<String, Value>,
    pub mcp_tools: Arc<tokio::sync::Mutex<Vec<(String, McpTool)>>>,
    pub spawn_tx: tokio::sync::broadcast::Sender<String>,
    // HistorySpell
    pub db: Db,
    pub embed: EmbeddingClient,
    pub conversation_id: Uuid,
    // ManageMcpSpell
    pub tool_inject_tx: tokio::sync::mpsc::UnboundedSender<ToolInjection>,
    pub pool: sqlx::PgPool,
    pub user_id: Uuid,
    // Shared
    pub abort_flag: Arc<AtomicBool>,
}

/// Build the complete built-in spell bundle from the given dependencies.
/// Returns a `ToolBundle` ready to be passed to `builder.add_tool(...)`.
pub fn build_all_spells(deps: SpellDeps) -> ToolBundle {
    let bundle = ToolBundle::new()
        .add(SkillSpell { skills_dir: std::path::PathBuf::from("/srv/familiar/skills") });

    bundle.add(A2aSpell)
        .add(UiSpells {
            ask_pending: deps.ask_pending,
        })
        .add(SpawnSpell {
            api_key: deps.api_key,
            api_base: deps.api_base,
            model_name: deps.model_name,
            extra_body: deps.extra_body,
            subagent_prompt: deps.subagent_prompt,
            mcp_tools: Arc::clone(&deps.mcp_tools),
            broadcast_tx: deps.spawn_tx,
            abort_flag: Arc::clone(&deps.abort_flag),
        })
        .add(HistorySpell {
            db: deps.db,
            embed: deps.embed,
            conversation_id: deps.conversation_id,
        })
        .add(ManageMcpSpell {
            mcp_tools: deps.mcp_tools,
            tool_inject_tx: deps.tool_inject_tx,
            pool: deps.pool,
            user_id: deps.user_id,
        })
}
