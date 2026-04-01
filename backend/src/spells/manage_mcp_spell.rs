use crate::config::McpCatalogEntry;
use agentix::McpTool;
use agentix::tool;
use serde_json::json;
use sqlx::PgPool;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::timeout;
use uuid::Uuid;

/// ManageMcpSpell — pure DB writes.
/// Install/uninstall writes to `conversation_mcps`.
/// The worker loads MCPs from DB at startup of each generation job.
pub struct ManageMcpSpell {
    pub pool: PgPool,
    pub conversation_id: Uuid,
    pub user_id: Uuid,
    pub sandbox: Arc<crate::sandbox::SandboxManager>,
    pub catalog: Vec<McpCatalogEntry>,
}

#[tool]
impl Tool for ManageMcpSpell {
    /// 列出系统配置中预设的可用 MCP 服务器（可直接安装，无需手动填写参数）
    async fn list_available_mcp(&self) -> Value {
        let entries: Vec<Value> = self
            .catalog
            .iter()
            .map(|entry| {
                json!({
                    "name": entry.name,
                    "description": entry.description,
                    "command": entry.command,
                    "args": entry.args
                })
            })
            .collect();
        json!({ "catalog": entries })
    }

    /// 列出当前会话已安装的 MCP 服务器配置
    async fn list_installed_mcp(&self) -> Value {
        let rows: Vec<(String, String, serde_json::Value)> = sqlx::query_as(
            r#"SELECT name, "type", config FROM conversation_mcps WHERE conversation_id = $1 ORDER BY name ASC"#,
        )
        .bind(self.conversation_id)
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default();

        let entries: Vec<Value> = rows
            .iter()
            .map(|(name, mcp_type, config)| {
                json!({
                    "name": name,
                    "type": mcp_type,
                    "config": config,
                })
            })
            .collect();
        json!({ "installed": entries })
    }

    /// 安装并激活 HTTP MCP 服务器。使用 list_available_mcp 查看可用预设。
    /// name: 服务器唯一标识符（用于后续卸载）
    /// url: MCP Streamable HTTP 端点 URL
    async fn install_mcp_http(&self, name: String, url: String) -> Value {
        // Validate connectivity before persisting.
        let tool = match timeout(Duration::from_secs(15), McpTool::http(&url)).await {
            Ok(Ok(t)) => t,
            Ok(Err(e)) => return json!({ "error": format!("启动失败: {e}") }),
            Err(_) => {
                return json!({ "error": format!("连接超时（15s），请检查服务器地址是否可访问: {url}") });
            }
        };
        let tool_count = tool.raw_tools().len();
        let new_tools = tool.raw_tools().clone();

        let config = json!({ "url": url });
        if let Err(e) = sqlx::query(
            r#"INSERT INTO conversation_mcps (conversation_id, name, "type", config) VALUES ($1, $2, 'http', $3)
               ON CONFLICT (conversation_id, name) DO UPDATE SET "type" = 'http', config = $3"#,
        )
        .bind(self.conversation_id)
        .bind(&name)
        .bind(&config)
        .execute(&self.pool)
        .await
        {
            return json!({ "error": format!("保存失败: {e}") });
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
        let args_ref: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        let (cmd, args_wrapped_vec) =
            self.sandbox
                .wrap_mcp_command(self.user_id, self.conversation_id, &command, &args_ref);
        let args_wrapped: Vec<&str> = args_wrapped_vec.iter().map(|s| s.as_str()).collect();

        // Validate that the command actually starts before persisting.
        let tool = match timeout(
            Duration::from_secs(300),
            McpTool::stdio(&cmd, &args_wrapped),
        )
        .await
        {
            Ok(Ok(t)) => t,
            Ok(Err(e)) => return json!({ "error": format!("启动失败: {e}") }),
            Err(_) => {
                return json!({ "error": format!("启动超时（300s），请检查命令是否有效: {command}") });
            }
        };
        let tool_count = tool.raw_tools().len();
        let new_tools = tool.raw_tools().clone();

        let config = json!({ "command": command, "args": args });
        if let Err(e) = sqlx::query(
            r#"INSERT INTO conversation_mcps (conversation_id, name, "type", config) VALUES ($1, $2, 'stdio', $3)
               ON CONFLICT (conversation_id, name) DO UPDATE SET "type" = 'stdio', config = $3"#,
        )
        .bind(self.conversation_id)
        .bind(&name)
        .bind(&config)
        .execute(&self.pool)
        .await
        {
            return json!({ "error": format!("保存失败: {e}") });
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
        let res =
            sqlx::query("DELETE FROM conversation_mcps WHERE conversation_id = $1 AND name = $2")
                .bind(self.conversation_id)
                .bind(&name)
                .execute(&self.pool)
                .await;

        match res {
            Ok(r) if r.rows_affected() > 0 => {
                json!({ "status": "ok", "message": format!("MCP '{}' 已卸载", name) })
            }
            Ok(_) => json!({ "error": format!("MCP '{}' 未在当前会话中安装", name) }),
            Err(e) => json!({ "error": format!("卸载失败: {e}") }),
        }
    }
}
