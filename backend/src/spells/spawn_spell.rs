use std::sync::Arc;

use crate::config::{ModelConfig, Provider};

use agentix::{AgentEvent, Message, Tool, ToolOutput, UserContent, tool};
use serde_json::json;

pub struct SpawnSpell {
    pub cheap_model: ModelConfig,
    /// Pre-rendered system prompt for fresh-mode subagents.
    pub fresh_prompt: String,
    /// Pre-rendered system prompt for fork-mode subagents.
    pub fork_prompt: String,
    pub tools: Arc<dyn Tool>,
    /// Conversation history for fork mode — agent inherits full context.
    pub history: Vec<Message>,
}

#[tool]
impl Tool for SpawnSpell {
    /// 启动子 Agent 完成子目标。有两种模式：
    ///
    /// **fresh 模式**（默认）：子 Agent 拥有全新空白上下文，从 goal 出发开始执行。
    /// 适合：与当前对话无关的独立任务、大量搜索/fetch/探索、不希望污染主上下文的中间过程。
    /// 使用时：goal 必须是完整自洽的 brief，因为子 Agent 看不到当前对话内容。
    ///
    /// **fork 模式**（fork=true）：子 Agent 继承当前完整对话上下文（prompt cache 共享）。
    /// 适合：需要理解当前对话状态才能执行的任务（如：「根据我们刚才讨论的修改方案，执行文件编辑」）。
    /// 使用时：directive 只需简短指令，不需要重述背景（子 Agent 已有全部上下文）。
    ///
    /// **什么时候用 fork vs fresh：**
    /// - fork：「继续/执行/基于上面的…」类任务，子 Agent 需要看到对话历史才能理解任务
    /// - fresh：独立的搜索、调研、文件处理，任务可以用一段文字完整描述
    ///
    /// description: 本次操作意图（供 UI 渲染，可不填）
    /// goal: fresh 模式下的完整任务目标；fork 模式下为简短指令（子 Agent 已有上下文）
    /// fork: 若为 true，子 Agent 继承当前对话上下文（默认 false）
    /// reasoner: 若为 true 则使用 deepseek-reasoner，适合复杂推理任务（默认 false）
    #[streaming]
    fn spawn(
        &self,
        description: Option<String>,
        goal: String,
        fork: Option<bool>,
        reasoner: Option<bool>,
    ) {
        let _ = description;

        let model_name =
            if reasoner == Some(true) && self.cheap_model.provider == Provider::DeepSeek {
                "deepseek-reasoner".to_owned()
            } else {
                self.cheap_model.name.clone()
            };

        let is_fork = fork.unwrap_or(false);
        let subagent_prompt = if is_fork {
            self.fork_prompt.clone()
        } else {
            self.fresh_prompt.clone()
        };
        let request = self
            .cheap_model
            .to_request()
            .model(model_name)
            .system_prompt(subagent_prompt);
        let history = if is_fork {
            // Fork: inherit conversation history, append the directive as the final user turn
            let mut h = self.history.clone();
            h.push(Message::User(vec![UserContent::Text { text: goal }]));
            h
        } else {
            // Fresh: blank context, goal is the only message
            vec![Message::User(vec![UserContent::Text { text: goal }])]
        };

        let http = reqwest::Client::new();
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
