use crate::config::Config;
use ds_api::{McpTool, ToolInjection};
use ds_api::tool;
use serde_json::{Value, json};
use sqlx::PgPool;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::sync::mpsc::UnboundedSender;
use uuid::Uuid;

pub struct ManageMcpSpell {
    /// All currently running MCP tools for this user's session.
    pub mcp_tools: Arc<Mutex<Vec<(String, McpTool)>>>,
    /// Channel to inject/remove tools in the running agent without rebuilding it.
    pub tool_inject_tx: UnboundedSender<ToolInjection>,
    /// DB pool for persisting MCP config.
    pub pool: PgPool,
    /// The authenticated user's id.
    pub user_id: Uuid,
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

    /// 安装并激活 HTTP MCP 服务器。使用 list_available_mcp 查看可用预设。
    /// name: 服务器唯一标识符（用于后续卸载）
    /// url: MCP Streamable HTTP 端点 URL
    async fn install_mcp_http(&self, name: String, url: String) -> Value {
        {
            let tools = self.mcp_tools.lock().await;
            if tools.iter().any(|(n, _)| n == &name) {
                return json!({ "error": format!("MCP '{}' 已在运行，请先卸载", name) });
            }
        }

        let tool = match McpTool::http(&url).await {
            Ok(t) => t,
            Err(e) => return json!({ "error": format!("启动失败: {e}") }),
        };
        let tool_count = tool.raw_tools().len();
        let new_tools = tool.raw_tools().clone();

        // Persist to DB so it survives restarts.
        let config = json!({ "url": url });
        if let Err(e) = sqlx::query(
            r#"INSERT INTO user_mcps (user_id, name, "type", config) VALUES ($1, $2, 'http', $3)
               ON CONFLICT (user_id, name) DO UPDATE SET "type" = 'http', config = $3"#
        )
        .bind(self.user_id)
        .bind(&name)
        .bind(&config)
        .execute(&self.pool)
        .await
        {
            tracing::warn!("failed to persist MCP '{}': {e}", name);
        }

        let _ = self.tool_inject_tx.send(ToolInjection::Add(Box::new(tool.clone())));
        {
            let mut tools = self.mcp_tools.lock().await;
            tools.push((name.clone(), tool));
        }

        json!({
            "status": "ok",
            "message": format!("MCP '{}' 已安装（{} 个工具），下一轮对话即可直接使用。", name, tool_count),
            "tools": new_tools
        })
    }

    /// 安装并激活 stdio MCP 服务器。使用 list_available_mcp 查看可用预设。
    /// name: 服务器唯一标识符（用于后续卸载）
    /// command: 启动命令（如 npx、uvx）
    /// args: 命令参数列表
    async fn install_mcp_stdio(&self, name: String, command: String, args: Vec<String>) -> Value {
        {
            let tools = self.mcp_tools.lock().await;
            if tools.iter().any(|(n, _)| n == &name) {
                return json!({ "error": format!("MCP '{}' 已在运行，请先卸载", name) });
            }
        }

        let args_ref: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        let tool = match McpTool::stdio(&command, &args_ref).await {
            Ok(t) => t,
            Err(e) => return json!({ "error": format!("启动失败: {e}") }),
        };
        let tool_count = tool.raw_tools().len();
        let new_tools = tool.raw_tools().clone();

        let config = json!({ "command": command, "args": args });
        if let Err(e) = sqlx::query(
            r#"INSERT INTO user_mcps (user_id, name, "type", config) VALUES ($1, $2, 'stdio', $3)
               ON CONFLICT (user_id, name) DO UPDATE SET "type" = 'stdio', config = $3"#
        )
        .bind(self.user_id)
        .bind(&name)
        .bind(&config)
        .execute(&self.pool)
        .await
        {
            tracing::warn!("failed to persist MCP '{}': {e}", name);
        }

        let _ = self.tool_inject_tx.send(ToolInjection::Add(Box::new(tool.clone())));
        {
            let mut tools = self.mcp_tools.lock().await;
            tools.push((name.clone(), tool));
        }

        json!({
            "status": "ok",
            "message": format!("MCP '{}' 已安装（{} 个工具），下一轮对话即可直接使用。", name, tool_count),
            "tools": new_tools
        })
    }

    /// 停止并卸载 MCP 服务器
    /// name: 要卸载的服务器标识符
    async fn uninstall_mcp(&self, name: String) -> Value {
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

        // Remove from DB.
        if let Err(e) = sqlx::query(
            "DELETE FROM user_mcps WHERE user_id = $1 AND name = $2"
        )
        .bind(self.user_id)
        .bind(&name)
        .execute(&self.pool)
        .await
        {
            tracing::warn!("failed to delete MCP '{}' from DB: {e}", name);
        }

        let _ = self.tool_inject_tx.send(ToolInjection::Remove(tool_names));
        {
            let mut tools = self.mcp_tools.lock().await;
            if let Some(idx) = tools.iter().position(|(n, _)| n == &name) {
                tools.remove(idx);
            }
        }

        json!({ "status": "ok", "message": format!("MCP '{}' 已卸载。", name) })
    }
}

// ── Config reading (catalog) ──────────────────────────────────────────────────

async fn read_catalog() -> anyhow::Result<Vec<Value>> {
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
