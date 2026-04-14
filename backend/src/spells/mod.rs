mod history_spell;
mod manage_mcp_spell;
mod memory_spell;
mod plan_spell;
mod sandbox_spell;
mod siliconflow_spell;
mod skill_spell;
mod sourcegraph_spell;
mod spawn_spell;
mod tavily_spell;
mod ui_spells;

use history_spell::HistorySpell;
use memory_spell::MemorySpell;
pub use memory_spell::consolidate_conversation_memories;
pub use memory_spell::load_memories_for_prompt;
use plan_spell::PlanSpell;
use sandbox_spell::SandboxSpell;
use siliconflow_spell::SiliconFlowSpell;
use skill_spell::SkillSpell;
use sourcegraph_spell::search_code;
use tavily_spell::TavilySpell;

use std::sync::Arc;
use uuid::Uuid;

use crate::config::{McpCatalogEntry, ModelConfig};
use crate::db::Db;
use crate::embedding::EmbeddingClient;
use crate::prompt::PromptEngine;
use manage_mcp_spell::ManageMcpSpell;
use spawn_spell::SpawnSpell;
use ui_spells::UiSpells;

// ── Spell factory ─────────────────────────────────────────────────────────────

/// All runtime dependencies required to build the full built-in spell bundle.
pub struct SpellDeps {
    pub prompt_engine: PromptEngine,
    pub current_date: String,
    // SpawnSpell
    pub cheap_model: ModelConfig,
    pub history: Vec<agentix::Message>,
    // HistorySpell
    pub db: Db,
    pub embed: EmbeddingClient,
    pub conversation_id: Uuid,
    // ManageMcpSpell
    pub pool: sqlx::PgPool,
    pub user_id: Uuid,
    pub sandbox: Arc<crate::sandbox::SandboxManager>,
    pub mcp_catalog: Vec<McpCatalogEntry>,
    // TavilySpell
    pub tavily_api_key: Option<String>,
    // SiliconFlowSpell
    pub siliconflow_api_key: Option<String>,
    pub http: reqwest::Client,
}

/// Build the complete built-in spell bundle from the given dependencies.
pub fn build_all_spells(deps: SpellDeps) -> impl agentix::Tool {
    // Build the subagent tool bundle first (no SpawnSpell — avoids infinite recursion).
    let subagent_tavily: Option<TavilySpell> =
        deps.tavily_api_key.as_deref().map(|k| TavilySpell {
            api_key: k.to_string(),
            http: deps.http.clone(),
        });

    let mut subagent_bundle = agentix::ToolBundle::new();
    subagent_bundle.push(
        SkillSpell {
            pool: deps.pool.clone(),
            user_id: deps.user_id,
        } + search_code
            + HistorySpell {
                db: deps.db.clone(),
                embedding: deps.embed.clone(),
                conversation_id: deps.conversation_id,
            }
            + PlanSpell {
                pool: deps.pool.clone(),
                conversation_id: deps.conversation_id,
            }
            + MemorySpell {
                pool: deps.pool.clone(),
                user_id: deps.user_id,
                conversation_id: deps.conversation_id,
                embed: deps.embed.clone(),
            },
    );
    if let Some(t) = subagent_tavily {
        subagent_bundle.push(t);
    }
    let subagent_tools: Arc<dyn agentix::Tool> = Arc::new(subagent_bundle);

    let bundle = SkillSpell {
        pool: deps.pool.clone(),
        user_id: deps.user_id,
    } + SandboxSpell {
        sandbox: deps.sandbox.clone(),
        user_id: deps.user_id,
        conversation_id: deps.conversation_id,
    } + UiSpells {
        user_id: deps.user_id,
        conversation_id: deps.conversation_id,
        sandbox: deps.sandbox.clone(),
    } + search_code
        + SpawnSpell {
            cheap_model: deps.cheap_model,
            fresh_prompt: deps.prompt_engine.build_subagent(false, &deps.current_date),
            fork_prompt: deps.prompt_engine.build_subagent(true, &deps.current_date),
            tools: subagent_tools,
            history: deps.history,
        }
        + HistorySpell {
            db: deps.db,
            embedding: deps.embed.clone(),
            conversation_id: deps.conversation_id,
        }
        + ManageMcpSpell {
            pool: deps.pool.clone(),
            conversation_id: deps.conversation_id,
            user_id: deps.user_id,
            sandbox: deps.sandbox.clone(),
            catalog: deps.mcp_catalog,
        }
        + PlanSpell {
            pool: deps.pool.clone(),
            conversation_id: deps.conversation_id,
        }
        + MemorySpell {
            pool: deps.pool,
            user_id: deps.user_id,
            conversation_id: deps.conversation_id,
            embed: deps.embed,
        };

    let mut tb = agentix::ToolBundle::new();
    tb.push(bundle);
    if let Some(api_key) = deps.tavily_api_key {
        tb.push(TavilySpell {
            api_key,
            http: deps.http.clone(),
        });
    }
    if let Some(api_key) = deps.siliconflow_api_key {
        tb.push(SiliconFlowSpell {
            api_key,
            http: deps.http.clone(),
            sandbox: deps.sandbox.clone(),
            user_id: deps.user_id,
            conversation_id: deps.conversation_id,
        });
    }
    tb
}
