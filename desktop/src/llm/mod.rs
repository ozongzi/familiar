pub mod anthropic;

use serde::Serialize;

use crate::storage::conversation::{Block, Conversation, Role};

/// Tool spec sent to the model.
#[derive(Debug, Clone, Serialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

/// Streaming events emitted to the UI as the model generates.
#[derive(Debug, Clone)]
pub enum Event {
    /// Assistant text delta.
    TextDelta(String),
    /// A tool_use block has started — id and name are known, input is being streamed.
    ToolUseStart { id: String, name: String },
    /// Partial JSON for the most recently started tool_use.
    ToolUseInputDelta(String),
    /// Stream finished naturally; reason indicates next step.
    Done(StopReason),
    /// Fatal error.
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopReason {
    /// Model wants to use one or more tools — caller should run them and continue.
    ToolUse,
    /// Model finished its turn.
    EndTurn,
    /// Stopped due to max_tokens or other limit.
    Other(String),
}

/// Convert our internal conversation into the message array Anthropic expects.
pub fn to_anthropic_messages(conv: &Conversation) -> Vec<serde_json::Value> {
    let mut out = Vec::with_capacity(conv.turns.len());
    for turn in &conv.turns {
        let role = match turn.role {
            Role::User => "user",
            Role::Assistant => "assistant",
        };
        let mut content = Vec::new();
        for block in &turn.blocks {
            match block {
                Block::Text { text } => {
                    if !text.is_empty() {
                        content.push(serde_json::json!({"type": "text", "text": text}));
                    }
                }
                Block::ToolUse { id, name, input } => {
                    content.push(serde_json::json!({
                        "type": "tool_use",
                        "id": id,
                        "name": name,
                        "input": input,
                    }));
                }
                Block::ToolResult { id, content: c, is_error } => {
                    content.push(serde_json::json!({
                        "type": "tool_result",
                        "tool_use_id": id,
                        "content": c,
                        "is_error": is_error,
                    }));
                }
            }
        }
        if !content.is_empty() {
            out.push(serde_json::json!({"role": role, "content": content}));
        }
    }
    out
}

