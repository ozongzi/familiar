mod a2a_spell;
mod file_spells;
mod history_spell;
mod manage_mcp_spell;
mod search_spells;
mod shell_spells;
mod spawn_spell;
mod ui_spells;

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

pub use ds_api::tool_trait::ToolBundle;
use ds_api::McpTool;
use serde_json::{Value, json};
use tokio::{process::Command, time::timeout};
use uuid::Uuid;

use a2a_spell::A2aSpell;
use file_spells::FileSpells;
use history_spell::HistorySpell;
use manage_mcp_spell::ManageMcpSpell;
use search_spells::SearchSpells;
use shell_spells::ShellSpells;
use spawn_spell::SpawnSpell;
use ui_spells::UiSpells;

use crate::db::Db;
use crate::embedding::EmbeddingClient;

pub const MAX_OUTPUT_CHARS: usize = 8_000;

/// 大文件自动降级到 outline 的行数阈值
pub(crate) const OUTLINE_THRESHOLD: usize = 300;

/// 超长输出保留头尾，中间用省略提示替换
pub(crate) fn truncate_output(s: &str, max_chars: usize) -> String {
    if s.len() <= max_chars {
        return s.to_string();
    }
    let half = max_chars / 2;
    let head = &s[..half];
    let tail_start = s.len() - half;
    let tail = &s[tail_start..];
    format!(
        "{}\n\n... [输出过长，中间 {} 字节已省略] ...\n\n{}",
        head,
        s.len() - max_chars,
        tail
    )
}

pub(crate) async fn run_cmd(mut cmd: Command, timeout_time: Duration) -> Value {
    cmd.kill_on_drop(true);
    match timeout(timeout_time, cmd.output()).await {
        Err(_) => json!({ "error": "命令超时" }),
        Ok(Err(e)) => json!({ "error": e.to_string() }),
        Ok(Ok(out)) => json!({
            "stdout": truncate_output(String::from_utf8_lossy(&out.stdout).trim(), MAX_OUTPUT_CHARS),
            "stderr": truncate_output(String::from_utf8_lossy(&out.stderr).trim(), MAX_OUTPUT_CHARS),
            "exit_code": out.status.code(),
        }),
    }
}

pub(crate) use search_spells::outline_value;

// ── Spell factory ─────────────────────────────────────────────────────────────

/// All runtime dependencies required to build the full built-in spell bundle.
/// Pass to `build_all_spells` in `build_agent`; no spell type needs to be
/// imported outside this module.
pub struct SpellDeps {
    // UiSpells
    pub ask_pending: Arc<tokio::sync::Mutex<Option<tokio::sync::oneshot::Sender<String>>>>,
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
    pub agent_stale: Arc<AtomicBool>,
}

/// Build the complete built-in spell bundle from the given dependencies.
/// Returns a `ToolBundle` ready to be passed to `builder.add_tool(...)`.
pub fn build_all_spells(deps: SpellDeps) -> ToolBundle {
    ToolBundle::new()
        .add(FileSpells)
        .add(ShellSpells)
        .add(SearchSpells)
        .add(A2aSpell)
        .add(UiSpells {
            ask_pending: deps.ask_pending,
        })
        .add(SpawnSpell {
            api_key: deps.api_key,
            api_base: deps.api_base,
            model_name: deps.model_name,
            extra_body: deps.extra_body,
            mcp_tools: Arc::clone(&deps.mcp_tools),
            broadcast_tx: deps.spawn_tx,
        })
        .add(HistorySpell {
            db: deps.db,
            embed: deps.embed,
            conversation_id: deps.conversation_id,
        })
        .add(ManageMcpSpell {
            mcp_tools: deps.mcp_tools,
            agent_stale: deps.agent_stale,
        })
}

async fn count_lines(path: &str) -> usize {
    Command::new("wc")
        .args(["-l", path])
        .output()
        .await
        .ok()
        .and_then(|o| {
            String::from_utf8_lossy(&o.stdout)
                .split_whitespace()
                .next()?
                .parse()
                .ok()
        })
        .unwrap_or(0)
}