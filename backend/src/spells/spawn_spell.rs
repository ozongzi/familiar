use std::{sync::Arc, sync::atomic::AtomicBool};

use crate::config::ModelConfig;

#[allow(unused)]
use super::a2a_spell::A2aSpell;
use ds_api::{AgentEvent, DeepseekAgent, McpTool, tool, tool_trait::ToolBundle};
use futures::StreamExt;
use serde_json::json;
use tokio::sync::Mutex;

pub struct SpawnSpell {
    pub cheap_model: ModelConfig,
    pub subagent_prompt: Option<String>,
    /// 与主 Agent 共享的 MCP 工具列表，子 Agent 全部继承
    pub mcp_tools: Arc<Mutex<Vec<(String, McpTool)>>>,
    /// 子 Agent 事件广播频道，供 UI 实时显示子 Agent 输出和工具调用
    pub broadcast_tx: tokio::sync::broadcast::Sender<String>,
    /// 主 Agent 的中断标志，子 Agent 共享
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

        let mut builder = DeepseekAgent::custom(
            self.cheap_model.api_key.clone(),
            self.cheap_model.api_base.clone(),
            if reasoner == Some(true) {
                "deepseek-reasoner".to_owned()
            } else {
                self.cheap_model.name.clone()
            },
        )
        .with_streaming()
        .with_system_prompt(self.subagent_prompt.clone().unwrap_or("".to_string()))
        .add_tool(
            ToolBundle::new(), // .add(A2aSpell)
        );

        for (k, v) in &self.cheap_model.extra_body {
            builder = builder.extra_field(k.clone(), v.clone());
        }

        for (_, tool) in mcp_snapshot {
            builder = builder.add_tool(tool);
        }

        // 保留 interrupt channel，但通过 abort_flag 检查中断
        let mut stream = builder.chat(&goal);

        let abort_flag = Arc::clone(&self.abort_flag);
        let mut result = String::new();
        while let Some(event) = stream.next().await {
            // 检查主 Agent 是否被中断
            if abort_flag.load(std::sync::atomic::Ordering::Acquire) {
                return json!({ "error": "任务被用户中断" });
            }

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
