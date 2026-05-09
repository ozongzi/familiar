use anyhow::{Context, Result, bail};
use eventsource_stream::Eventsource;
use futures::StreamExt;
use tokio::sync::mpsc;

use super::{Event, StopReason, ToolSpec, to_anthropic_messages};
use crate::storage::conversation::Conversation;

const API_URL: &str = "https://api.anthropic.com/v1/messages";
const API_VERSION: &str = "2023-06-01";

pub struct StreamRequest {
    pub api_key: String,
    pub model: String,
    pub max_tokens: u32,
    pub system: String,
    pub conversation: Conversation,
    pub tools: Vec<ToolSpec>,
}

/// Open a streaming /v1/messages request and forward events on the channel.
/// Returns when the stream closes; callers should drive the loop themselves
/// based on `Event::Done(StopReason)`.
pub async fn stream(req: StreamRequest, tx: mpsc::UnboundedSender<Event>) -> Result<()> {
    let messages = to_anthropic_messages(&req.conversation);
    let body = serde_json::json!({
        "model": req.model,
        "max_tokens": req.max_tokens,
        "system": req.system,
        "messages": messages,
        "tools": req.tools,
        "stream": true,
    });

    let client = reqwest::Client::builder()
        .build()
        .context("build http client")?;

    let resp = client
        .post(API_URL)
        .header("x-api-key", &req.api_key)
        .header("anthropic-version", API_VERSION)
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .context("send request")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        bail!("anthropic API error {status}: {text}");
    }

    let mut stream = resp.bytes_stream().eventsource();
    while let Some(item) = stream.next().await {
        let evt = match item {
            Ok(e) => e,
            Err(e) => {
                let _ = tx.send(Event::Error(format!("stream: {e}")));
                break;
            }
        };

        // Anthropic uses event types like "message_start", "content_block_start",
        // "content_block_delta", "content_block_stop", "message_delta", "message_stop".
        let data: serde_json::Value = match serde_json::from_str(&evt.data) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let kind = data.get("type").and_then(|v| v.as_str()).unwrap_or("");

        match kind {
            "content_block_start" => {
                let block = data.get("content_block").cloned().unwrap_or_default();
                let block_type =
                    block.get("type").and_then(|v| v.as_str()).unwrap_or("").to_string();
                if block_type == "tool_use" {
                    let id =
                        block.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let name = block
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let _ = tx.send(Event::ToolUseStart { id, name });
                }
            }
            "content_block_delta" => {
                let delta = data.get("delta").cloned().unwrap_or_default();
                let dtype =
                    delta.get("type").and_then(|v| v.as_str()).unwrap_or("").to_string();
                match dtype.as_str() {
                    "text_delta" => {
                        if let Some(t) = delta.get("text").and_then(|v| v.as_str()) {
                            let _ = tx.send(Event::TextDelta(t.to_string()));
                        }
                    }
                    "input_json_delta" => {
                        if let Some(t) = delta.get("partial_json").and_then(|v| v.as_str()) {
                            let _ = tx.send(Event::ToolUseInputDelta(t.to_string()));
                        }
                    }
                    _ => {}
                }
            }
            "message_delta" => {
                let stop = data
                    .get("delta")
                    .and_then(|d| d.get("stop_reason"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                if let Some(reason) = stop {
                    let parsed = match reason.as_str() {
                        "tool_use" => StopReason::ToolUse,
                        "end_turn" => StopReason::EndTurn,
                        other => StopReason::Other(other.to_string()),
                    };
                    let _ = tx.send(Event::Done(parsed));
                }
            }
            "error" => {
                let msg = data
                    .get("error")
                    .and_then(|e| e.get("message"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown error")
                    .to_string();
                let _ = tx.send(Event::Error(msg));
                break;
            }
            _ => {}
        }
    }

    Ok(())
}
