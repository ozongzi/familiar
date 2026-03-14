use crate::config::Config;
use ds_api::{McpTool, ToolInjection};
use ds_api::tool;
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::sync::mpsc::UnboundedSender;

pub struct ManageMcpSpell {
    /// All currently running MCP tools, shared with AppState.
    pub mcp_tools: Arc<Mutex<Vec<(String, McpTool)>>>,
    /// Channel to inject/remove tools in the running agent without rebuilding it.
    pub tool_inject_tx: UnboundedSender<ToolInjection>,
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
        let tool_count = tool.raw_tools().len();
        let new_tools = tool.raw_tools().clone();

        // Inject into the running agent immediately so the next turn can use it.
        let _ = self.tool_inject_tx.send(ToolInjection::Add(Box::new(tool.clone())));

        // Also persist in the shared list so newly built agents include it.
        {
            let mut tools = self.mcp_tools.lock().await;
            tools.push((name.clone(), tool));
        }

        json!({
            "status": "ok",
            "message": format!("MCP '{}' 已安装（{} 个工具），下一轮对话即可直接使用。进程重启后不会自动恢复，若消失请重新安装。", name, tool_count),
            "tools": new_tools
        })
    }

    /// 停止并卸载 MCP 服务器
    /// name: 要卸载的服务器标识符
    async fn uninstall_mcp(&self, name: String) -> Value {
        // Find the tool names belonging to this MCP server, then remove from agent.
        let tool_names: Vec<String> = {
            let tools = self.mcp_tools.lock().await;
            tools
                .iter()
                .find(|(n, _)| n == &name)
                .map(|(_, t)| t.raw_tools().iter().map(|r| r.function.name.clone()).collect())
                .unwrap_or_default()
        };

        if tool_names.is_empty() {
            return json!({ "error": format!("MCP '{}' 未在运行列表中", name) });
        }

        // Remove from the running agent immediately.
        let _ = self.tool_inject_tx.send(ToolInjection::Remove(tool_names));

        // Remove from shared list (this kills the subprocess via Drop).
        {
            let mut tools = self.mcp_tools.lock().await;
            if let Some(idx) = tools.iter().position(|(n, _)| n == &name) {
                tools.remove(idx);
            }
        }

        json!({ "status": "ok", "message": format!("MCP '{}' 已卸载，下一轮对话生效。", name) })
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
