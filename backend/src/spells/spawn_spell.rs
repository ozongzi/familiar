use std::{collections::HashMap, sync::Arc};

use ds_api::{AgentEvent, DeepseekAgent, McpTool, tool, tool_trait::ToolBundle};
use futures::StreamExt;
use serde_json::{Value, json};
use tokio::sync::Mutex;
use super::a2a_spell::A2aSpell;
use super::file_spells::FileSpells;
use super::search_spells::SearchSpells;
use super::shell_spells::ShellSpells;

pub struct SpawnSpell {
    pub api_key: String,
    pub api_base: String,
    pub model_name: String,
    pub extra_body: HashMap<String, Value>,
    pub subagent_prompt: Option<String>,
    /// 与主 Agent 共享的 MCP 工具列表，子 Agent 全部继承
    pub mcp_tools: Arc<Mutex<Vec<(String, McpTool)>>>,
    /// 子 Agent 事件广播频道，供 UI 实时显示子 Agent 输出和工具调用
    pub broadcast_tx: tokio::sync::broadcast::Sender<String>,
}

#[tool]
impl Tool for SpawnSpell {
    /// 启动独立子 Agent 完成子目标，子 Agent 有独立上下文，跑完返回结果摘要。
    /// 适合大量搜索 / fetch / 探索但不希望污染主上下文的任务（如 Search Agent）。
    /// 子 Agent 拥有与主 Agent 相同的工具集（file / shell / search / a2a + 所有 MCP）。
    ///
    /// description: 本次操作意图（供 UI 渲染，可不填）
    /// goal: 子 Agent 的目标，尽量具体
    async fn spawn(&self, description: Option<String>, goal: String) -> Value {
        let _ = description;
        let mcp_snapshot: Vec<(String, McpTool)> = self.mcp_tools.lock().await.clone();

        let mut builder = DeepseekAgent::custom(
            self.api_key.clone(),
            self.api_base.clone(),
            self.model_name.clone(),
        )
            .with_streaming()
            .with_system_prompt(
                self.subagent_prompt.clone().unwrap_or("".to_string()),
            )
            .add_tool(
                ToolBundle::new()
                    .add(FileSpells)
                    .add(ShellSpells)
                    .add(SearchSpells)
                    .add(A2aSpell),
            );

        for (k, v) in &self.extra_body {
            builder = builder.extra_field(k.clone(), v.clone());
        }

        for (_, tool) in mcp_snapshot {
            builder = builder.add_tool(tool);
        }

        let (agent, _interrupt_tx) = builder.with_interrupt_channel();
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