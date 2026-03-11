use crate::config::Config;
use ds_api::McpTool;
use ds_api::tool;
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::Mutex;

pub struct ManageMcpSpell {
    /// All currently running MCP tools, shared with AppState.
    pub mcp_tools: Arc<Mutex<Vec<(String, McpTool)>>>,
    /// Set to true after install/uninstall so run_generation drops the
    /// recovered agent; the next start_generation will rebuild it with the
    /// updated tool list.
    pub agent_stale: Arc<AtomicBool>,
    /// Sum of raw_tools().len() for all built-in spells. Used for limit check.
    pub builtin_tool_count: usize,
    /// Maximum total tool definitions (built-in + all MCP).
    pub max_tools: usize,
}

#[tool]
impl Tool for ManageMcpSpell {
    /// 列出 config.toml 中预设的可用 MCP 服务器（可直接安装，无需手动填写参数）
    async fn list_available_mcp(&self) -> Value {
        match read_catalog().await {
            Ok(entries) => json!({ "catalog": entries }),
            Err(e) => json!({ "catalog": [], "error": e.to_string() }),
        }
    }

    /// 列出当前已安装并运行的 MCP 服务器名称
    async fn list_installed_mcp(&self) -> Value {
        let tools = self.mcp_tools.lock().await;
        let entries: Vec<Value> = tools
            .iter()
            .map(|(name, tool)| json!({ "name": name, "tool_count": tool.raw_tools().len() }))
            .collect();
        json!({ "installed": entries })
    }

    /// 安装并激活 MCP 服务器。使用 list_available_mcp 查看可用预设。
    /// name: 服务器唯一标识符（用于后续卸载）
    /// command: 启动命令（如 npx、uvx、mcp-language-server）
    /// args: 命令参数列表
    async fn install_mcp(&self, name: String, command: String, args: Vec<String>) -> Value {
        // Duplicate check
        {
            let tools = self.mcp_tools.lock().await;
            if tools.iter().any(|(n, _)| n == &name) {
                return json!({ "error": format!("MCP '{}' 已在运行，请先卸载", name) });
            }
        }

        // Start the subprocess
        let args_ref: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        let tool = match McpTool::stdio(&command, &args_ref).await {
            Ok(t) => t,
            Err(e) => return json!({ "error": format!("启动失败: {e}") }),
        };
        let new_tool_count = tool.raw_tools().len();

        // Limit check: count existing MCP tools + new ones + built-in
        {
            let tools = self.mcp_tools.lock().await;
            let mcp_total: usize = tools.iter().map(|(_, t)| t.raw_tools().len()).sum();
            let total = self.builtin_tool_count + mcp_total + new_tool_count;
            if total > self.max_tools {
                return json!({
                    "error": format!(
                        "安装后工具总数 {} 将超过上限 {}（内置 {} + 现有 MCP {} + 新增 {}）",
                        total, self.max_tools,
                        self.builtin_tool_count, mcp_total, new_tool_count
                    )
                });
            }
        }

        // Add to running tools
        {
            let mut tools = self.mcp_tools.lock().await;
            tools.push((name.clone(), tool));
        }

        // Mark agent stale — next generation rebuilds with updated MCP list
        self.agent_stale.store(true, Ordering::Relaxed);

        // NOTE: persistence to config.toml has been removed. MCPs installed this way
        // are active for the current process lifetime only.

        json!({
            "status": "ok",
            "message": format!("MCP '{}' 已安装（{} 个工具）。此安装为即时生效，进程重启后不会自动恢复。", name, new_tool_count)
        })
    }

    /// 停止并卸载 MCP 服务器
    /// name: 要卸载的服务器标识符
    async fn uninstall_mcp(&self, name: String) -> Value {
        let removed = {
            let mut tools = self.mcp_tools.lock().await;
            if let Some(idx) = tools.iter().position(|(n, _)| n == &name) {
                tools.remove(idx); // Drop kills the subprocess
                true
            } else {
                false
            }
        };

        if !removed {
            return json!({ "error": format!("MCP '{}' 未在运行列表中", name) });
        }

        self.agent_stale.store(true, Ordering::Relaxed);

        // NOTE: persistence to config.toml has been removed. Uninstall only affects
        // the running process; no on-disk config is modified.

        json!({ "status": "ok", "message": format!("MCP '{}' 已卸载。", name) })
    }
}

// ── Config reading (catalog) ──────────────────────────────────────────────────

async fn read_catalog() -> anyhow::Result<Vec<Value>> {
    // Load configuration using the central Config loader so we respect env overrides.
    let cfg = Config::load();

    let entries: Vec<Value> = cfg
        .mcp_catalog
        .into_iter()
        .map(|entry| {
            json!({
                "name": entry.name,
                "description": entry.description,
                "command": entry.command,
                "args": entry.args
            })
        })
        .collect();

    Ok(entries)
}
