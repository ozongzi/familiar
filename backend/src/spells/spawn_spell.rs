use std::sync::Arc;

use ds_api::{AgentEvent, DeepseekAgent, McpTool, tool};
use futures::StreamExt;
use serde_json::json;
use tokio::sync::Mutex;

pub struct SpawnSpell {
    pub api_key: String,
    pub api_base: String,
    pub model_name: String,
    /// 可向子 Agent 注入的 MCP 工具快照
    pub mcp_tools: Arc<Mutex<Vec<(String, McpTool)>>>,
    /// 默认安全工具白名单（无副作用：search/glob/outline/read 等）
    pub default_tools: Vec<String>,
    /// 子 Agent 事件广播频道，供 UI 实时显示子 Agent 输出和工具调用
    pub broadcast_tx: tokio::sync::broadcast::Sender<String>,
}

#[tool]
impl Tool for SpawnSpell {
    /// 启动独立子 Agent 完成子目标，子 Agent 有独立上下文，跑完返回结果摘要。
    /// 适合大量搜索 / fetch / 探索但不希望污染主上下文的任务（如 Search Agent）。
    /// 子 Agent 默认只能用无副作用工具；需要写文件时须在 tools 中显式列出。
    ///
    /// description: 本次操作意图（供 UI 渲染，可不填）
    /// goal: 子 Agent 的目标，尽量具体
    /// tools: 允许使用的工具名列表（可选，不填则用默认安全集）
    async fn spawn(
        &self,
        description: Option<String>,
        goal: String,
        tools: Option<Vec<String>>,
    ) -> Value {
        let _ = description;
        let allowed: Vec<String> = tools.unwrap_or_else(|| self.default_tools.clone());
        let mcp_snapshot: Vec<(String, McpTool)> = self.mcp_tools.lock().await.clone();

        let mut builder = DeepseekAgent::custom(
            self.api_key.clone(),
            self.api_base.clone(),
            self.model_name.clone(),
        )
        .with_streaming()
        .with_system_prompt(
            r#"你是专注完成单一子任务的 Agent。
             完成后直接输出结果摘要，不要闲聊。
             只使用被授权的工具。
             请始终用中文回复。"#
                .to_string(),
        );

        for tool in &mcp_snapshot {
            if allowed.iter().any(|a| a == &tool.0 || a == "*") {
                builder = builder.add_tool(tool.1.clone());
            }
        }

        let (agent, _) = builder.with_interrupt_channel();
        let mut stream = agent.chat(&goal);

        let mut result = String::new();
        while let Some(event) = stream.next().await {
            match event {
                Ok(AgentEvent::Token(t)) => {
                    result.push_str(&t);
                    let _ = self.broadcast_tx.send(
                        json!({
                            "type": "token",
                            "content": t,
                            "source": "spawn",
                        })
                        .to_string(),
                    );
                }
                Ok(AgentEvent::ToolCall(c)) => {
                    let _ = self.broadcast_tx.send(
                        json!({
                            "type": "tool_call",
                            "id": c.id,
                            "name": c.name,
                            "delta": c.delta,
                            "source": "spawn",
                        })
                        .to_string(),
                    );
                }
                Ok(AgentEvent::ToolResult(res)) => {
                    let _ = self.broadcast_tx.send(
                        json!({
                            "type": "tool_result",
                            "id": res.id,
                            "name": res.name,
                            "result": res.result,
                            "source": "spawn",
                        })
                        .to_string(),
                    );
                }
                Ok(AgentEvent::ReasoningToken(_)) => {}
                Err(e) => return json!({ "error": format!("子 Agent 错误: {e}") }),
            }
        }

        if result.is_empty() {
            json!({ "error": "子 Agent 未返回任何结果" })
        } else {
            json!({ "result": result })
        }
    }
}
