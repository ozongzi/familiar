use std::{sync::Arc, sync::atomic::AtomicBool};

use crate::config::ModelConfig;

use agentix::{Agent, AgentEvent, LlmClient, McpTool, tool};
use crate::config::Provider;
use serde_json::json;
use tokio::sync::Mutex;

pub struct SpawnSpell {
    pub cheap_model: ModelConfig,
    pub subagent_prompt: Option<String>,
    pub mcp_tools: Arc<Mutex<Vec<(String, McpTool)>>>,
    pub broadcast_tx: tokio::sync::broadcast::Sender<String>,
    pub abort_flag: Arc<AtomicBool>,
}

#[tool]
impl Tool for SpawnSpell {
    /// 启动独立子 Agent 完成子目标，使用 DeepSeek 模型，子 Agent 有独立上下文，跑完返回结果摘要。
    /// 适合大量搜索 / fetch / 探索但不希望污染主上下文的任务（如 Search Agent）。
    /// 子 Agent 拥有与主 Agent 相同的工具集（file / shell / search / a2a + 所有 MCP）。
    ///
    /// description: 本次操作意图（供 UI 渲染，可不填）
    /// goal: 子 Agent 的目标，尽量具体
    /// reasoner: 可选，默认为 false 若为 true 则使用 deepseek-reasoner 模型，适合需要复杂推理的子目标，否则使用 deepseek-chat 模型，适合一般对话和工具调用的子目标
    async fn spawn(
        &self,
        description: Option<String>,
        goal: String,
        reasoner: Option<bool>,
    ) -> Value {
        let _ = description;
        let mcp_snapshot: Vec<(String, McpTool)> = self.mcp_tools.lock().await.clone();

        let model_name = if reasoner == Some(true) && self.cheap_model.provider == Provider::DeepSeek {
            "deepseek-reasoner".to_owned()
        } else {
            self.cheap_model.name.clone()
        };

        let client = match self.cheap_model.provider {
            Provider::DeepSeek  => LlmClient::deepseek(self.cheap_model.api_key.clone()),
            Provider::OpenAI    => LlmClient::openai(self.cheap_model.api_key.clone()),
            Provider::Anthropic => LlmClient::anthropic(self.cheap_model.api_key.clone()),
            Provider::Gemini    => LlmClient::gemini(self.cheap_model.api_key.clone()),
        };
        client.base_url(self.cheap_model.api_base.clone());
        client.model(model_name);

        let mut agent = Agent::new(client);
        if let Some(prompt) = &self.subagent_prompt {
            agent = agent.system_prompt(prompt.clone());
        }

        let bundle: agentix::ToolBundle = mcp_snapshot.into_iter().map(|(_, t)| t).sum();
        agent = agent.tool(bundle);

        let abort_flag = Arc::clone(&self.abort_flag);
        let mut stream = match agent.chat(goal).await {
            Ok(s) => s,
            Err(e) => return json!({ "error": format!("子 Agent 启动失败: {e}") }),
        };

        let mut result = String::new();
        while let Some(event) = stream.next().await {
            if abort_flag.load(std::sync::atomic::Ordering::Acquire) {
                return json!({ "error": "任务被用户中断" });
            }

            match event {
                AgentEvent::Token(t) => {
                    result.push_str(&t);
                    let _ = self.broadcast_tx.send(
                        json!({"type": "token", "content": t, "source": "spawn"}).to_string(),
                    );
                }
                AgentEvent::ToolCallChunk(c) => {
                    let _ = self.broadcast_tx.send(
                        json!({"type": "tool_call", "id": c.id, "name": c.name, "delta": c.delta, "source": "spawn"}).to_string(),
                    );
                }
                AgentEvent::ToolCall(c) => {
                    let _ = self.broadcast_tx.send(
                        json!({"type": "tool_call", "id": c.id, "name": c.name, "delta": c.arguments, "source": "spawn"}).to_string(),
                    );
                }
                AgentEvent::ToolResult { call_id, name, result: res } => {
                    let _ = self.broadcast_tx.send(
                        json!({"type": "tool_result", "id": call_id, "name": name, "result": res, "source": "spawn"}).to_string(),
                    );
                }
                AgentEvent::Reasoning(_) | AgentEvent::Done => {}
                AgentEvent::Error(e) => return json!({ "error": format!("子 Agent 错误: {e}") }),
                _ => {}
            }
        }

        if result.is_empty() {
            json!({ "error": "子 Agent 未返回任何结果" })
        } else {
            json!({ "result": result })
        }
    }
}
