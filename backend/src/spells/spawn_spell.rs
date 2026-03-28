use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::config::ModelConfig;
use crate::config::Provider;

use agentix::{LlmEvent, Message, tool};
use serde_json::json;

pub struct SpawnSpell {
    pub cheap_model: ModelConfig,
    pub subagent_prompt: Option<String>,
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

        let model_name = if reasoner == Some(true) && self.cheap_model.provider == Provider::DeepSeek {
            "deepseek-reasoner".to_owned()
        } else {
            self.cheap_model.name.clone()
        };

        let mut request = self.cheap_model.to_request().model(model_name);
        if let Some(prompt) = &self.subagent_prompt {
            request = request.system_prompt(prompt.clone());
        }
        request = request.message(Message::User(vec![agentix::UserContent::Text(goal)]));

        let http = reqwest::Client::new();
        let mut stream = match request.stream(&http).await {
            Ok(s) => s,
            Err(e) => return json!({ "error": format!("子 Agent 启动失败: {e}") }),
        };

        let abort_flag = Arc::clone(&self.abort_flag);
        let mut result = String::new();

        while let Some(event) = stream.next().await {
            if abort_flag.load(std::sync::atomic::Ordering::Acquire) {
                return json!({ "error": "任务被用户中断" });
            }

            match event {
                LlmEvent::Token(t) => {
                    result.push_str(&t);
                    let _ = self.broadcast_tx.send(
                        json!({"type": "token", "content": t, "source": "spawn"}).to_string(),
                    );
                }
                LlmEvent::ToolCallChunk(c) => {
                    let _ = self.broadcast_tx.send(
                        json!({"type": "tool_call", "id": c.id, "name": c.name, "delta": c.delta, "source": "spawn"}).to_string(),
                    );
                }
                LlmEvent::Reasoning(t) => {
                    let _ = self.broadcast_tx.send(
                        json!({"type": "reasoning_token", "content": t, "source": "spawn"}).to_string(),
                    );
                }
                LlmEvent::Error(e) => return json!({ "error": format!("子 Agent 错误: {e}") }),
                LlmEvent::Done => break,
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
