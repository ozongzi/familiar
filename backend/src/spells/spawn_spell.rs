use std::sync::Arc;

use crate::config::{ModelConfig, Provider};

use agentix::{AgentEvent, Message, Tool, ToolOutput, tool};
use serde_json::json;

pub struct SpawnSpell {
    pub cheap_model: ModelConfig,
    pub subagent_prompt: Option<String>,
    pub tools: Arc<dyn Tool>,
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
    #[streaming]
    fn spawn(
        &self,
        description: Option<String>,
        goal: String,
        reasoner: Option<bool>,
    ) {
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

        let http = reqwest::Client::new();
        let history = vec![Message::User(vec![agentix::UserContent::Text { text: goal }])];
        let tools = Arc::clone(&self.tools);

        async_stream::stream! {
            use futures::StreamExt;

            let mut result = String::new();
            let mut agent_stream = agentix::agent(tools, http, request, history, Some(25_000));

            while let Some(event) = agent_stream.next().await {
                match event {
                    AgentEvent::Token(t) => {
                        result.push_str(&t);
                        yield ToolOutput::Progress(
                            json!({"type": "token", "content": t, "source": "spawn"}).to_string()
                        );
                    }
                    AgentEvent::Reasoning(t) => yield ToolOutput::Progress(
                        json!({"type": "reasoning_token", "content": t, "source": "spawn"}).to_string()
                    ),
                    AgentEvent::ToolCallChunk(c) => yield ToolOutput::Progress(
                        json!({"type": "tool_call", "id": c.id, "name": c.name, "delta": c.delta, "source": "spawn"}).to_string()
                    ),
                    AgentEvent::ToolCallStart(tc) => yield ToolOutput::Progress(
                        json!({"type": "tool_call_complete", "id": tc.id, "name": tc.name, "source": "spawn"}).to_string()
                    ),
                    AgentEvent::ToolProgress { id, name, progress } => yield ToolOutput::Progress(
                        json!({"type": "tool_progress", "id": id, "name": name, "progress": progress, "source": "spawn"}).to_string()
                    ),
                    AgentEvent::ToolResult { id, name, content } => yield ToolOutput::Progress(
                        json!({"type": "tool_result", "id": id, "name": name, "result": content, "source": "spawn"}).to_string()
                    ),
                    AgentEvent::Error(e) => {
                        yield ToolOutput::Result(json!({ "error": format!("子 Agent 错误: {e}") }));
                        return;
                    }
                    AgentEvent::Warning(_) | AgentEvent::Usage(_) | AgentEvent::Done(_) => {}
                }
            }

            if result.is_empty() {
                yield ToolOutput::Result(json!({ "error": "子 Agent 未返回任何结果" }));
            } else {
                yield ToolOutput::Result(json!({ "result": result }));
            }
        }
    }
}
