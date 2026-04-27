//! Agentix-backed streaming path for [`crate::client::ModelClient::stream`].
//!
//! When `wire_api = "agentix"` is set on a model provider, the turn is routed
//! through the [`agentix`] crate instead of the OpenAI Responses API. This
//! module owns the two-way translation:
//!
//! - **Inbound** (Codex → agentix): the [`Prompt`] (input items + tools +
//!   base instructions) is rebuilt as an [`agentix::Request`].
//! - **Outbound** (agentix → Codex): the [`agentix::msg::LlmEvent`] stream
//!   is mapped onto [`ResponseEvent`]s and pushed through the standard mpsc
//!   channel that `ModelClient::stream` returns.
//!
//! Cf. familiar's `backend/src/worker.rs` for the canonical hand-written
//! LlmEvent → consumer loop this module mirrors.

use agentix::msg::LlmEvent;
use agentix::raw::shared::FunctionDefinition;
use agentix::raw::shared::ToolDefinition;
use agentix::request::Content;
use agentix::request::ImageContent;
use agentix::request::ImageData;
use agentix::request::Message;
use agentix::request::Provider as AgentixProvider;
use agentix::request::Request;
use agentix::request::ToolCall as AgentixToolCall;
use agentix::types::UsageStats;
use codex_api::ResponseEvent;
use codex_protocol::error::CodexErr;
use codex_protocol::error::Result;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::TokenUsage;
use codex_tools::ToolSpec;
use futures::StreamExt;
use tokio::sync::mpsc;
use tracing::warn;

use crate::client_common::Prompt;
use crate::client_common::ResponseStream;

/// Channel capacity for `LlmEvent → ResponseEvent` adaptation. Mirrors the
/// 1600-slot buffer used by the existing Responses-API path.
const EVENT_CHANNEL_CAPACITY: usize = 1600;

/// Stream one model turn through agentix.
pub async fn stream_via_agentix(
    prompt: &Prompt,
    model_slug: &str,
    agentix_provider_id: &str,
    api_key: String,
) -> Result<ResponseStream> {
    let provider = parse_agentix_provider(agentix_provider_id)?;

    let messages = prompt_to_agentix_messages(prompt);
    let tools = tools_to_agentix_definitions(&prompt.tools);

    let request = Request::new(provider, api_key)
        .model(model_slug)
        .system_prompt(prompt.base_instructions.text.clone())
        .messages(messages)
        .tools(tools);

    let http = agentix_http_client::Client::new();
    let mut stream = request
        .stream(&http)
        .await
        .map_err(|e| CodexErr::Stream(format!("agentix stream open failed: {e}"), None))?;

    let (tx, rx) = mpsc::channel::<Result<ResponseEvent>>(EVENT_CHANNEL_CAPACITY);

    tokio::spawn(async move {
        let mut text_buf = String::new();
        let mut reasoning_buf = String::new();
        let mut tool_calls_buf: Vec<AgentixToolCall> = Vec::new();
        let mut usage: Option<UsageStats> = None;

        while let Some(event) = stream.next().await {
            match event {
                LlmEvent::Token(t) => {
                    text_buf.push_str(&t);
                    if tx.send(Ok(ResponseEvent::OutputTextDelta(t))).await.is_err() {
                        return;
                    }
                }
                LlmEvent::Reasoning(t) => {
                    reasoning_buf.push_str(&t);
                    if tx
                        .send(Ok(ResponseEvent::ReasoningContentDelta {
                            delta: t,
                            content_index: 0,
                        }))
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
                LlmEvent::ToolCallChunk(c) => {
                    if tx
                        .send(Ok(ResponseEvent::ToolCallInputDelta {
                            item_id: c.id.clone(),
                            call_id: Some(c.id),
                            delta: c.delta,
                        }))
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
                LlmEvent::ToolCall(tc) => {
                    tool_calls_buf.push(tc);
                }
                LlmEvent::Usage(u) => {
                    usage = Some(
                        usage
                            .take()
                            .map(|prev| accumulate_usage(prev, u.clone()))
                            .unwrap_or(u),
                    );
                }
                LlmEvent::AssistantState(_v) => {
                    // TODO: round-trip provider_data via a future ResponseItem
                    // variant or a side channel; codex's protocol has no
                    // direct equivalent today.
                }
                LlmEvent::Done => break,
                LlmEvent::Error(e) => {
                    let _ = tx.send(Err(CodexErr::Stream(e, None))).await;
                    return;
                }
                _ => {}
            }
        }

        // Drop tool calls whose arguments are not a complete JSON object —
        // some providers emit a trailing partial chunk on stream truncation
        // and downstream APIs reject incomplete arguments.
        tool_calls_buf.retain(|tc| {
            serde_json::from_str::<serde_json::Value>(&tc.arguments)
                .map(|v| v.is_object())
                .unwrap_or(false)
        });

        if !reasoning_buf.is_empty() {
            let item = ResponseItem::Reasoning {
                id: String::new(),
                summary: Vec::new(),
                content: None,
                encrypted_content: None,
            };
            if tx.send(Ok(ResponseEvent::OutputItemDone(item))).await.is_err() {
                return;
            }
        }

        if !text_buf.is_empty() {
            let item = ResponseItem::Message {
                id: None,
                role: "assistant".to_string(),
                content: vec![ContentItem::OutputText { text: text_buf }],
                phase: None,
            };
            if tx.send(Ok(ResponseEvent::OutputItemDone(item))).await.is_err() {
                return;
            }
        }

        for tc in tool_calls_buf {
            let item = ResponseItem::FunctionCall {
                id: None,
                name: tc.name,
                namespace: None,
                arguments: tc.arguments,
                call_id: tc.id,
            };
            if tx.send(Ok(ResponseEvent::OutputItemDone(item))).await.is_err() {
                return;
            }
        }

        let _ = tx
            .send(Ok(ResponseEvent::Completed {
                response_id: String::new(),
                token_usage: usage.map(usage_stats_to_token_usage),
                end_turn: Some(true),
            }))
            .await;
    });

    Ok(ResponseStream { rx_event: rx })
}

/// Parse the string id stored on `ModelProviderInfo::agentix_provider`.
fn parse_agentix_provider(id: &str) -> Result<AgentixProvider> {
    match id {
        "deepseek" => Ok(AgentixProvider::DeepSeek),
        "openai" => Ok(AgentixProvider::OpenAI),
        "anthropic" => Ok(AgentixProvider::Anthropic),
        "gemini" => Ok(AgentixProvider::Gemini),
        "kimi" => Ok(AgentixProvider::Kimi),
        "glm" => Ok(AgentixProvider::Glm),
        "minimax" => Ok(AgentixProvider::Minimax),
        "grok" => Ok(AgentixProvider::Grok),
        "openrouter" => Ok(AgentixProvider::OpenRouter),
        // ClaudeCode is gated behind `feature = "claude-code"` on the agentix
        // crate; we don't enable it in the default-features=false dependency.
        other => Err(CodexErr::Stream(
            format!("unknown agentix_provider id: {other:?}"),
            None,
        )),
    }
}

/// Walk Codex `ResponseItem`s in order and emit the agentix `Message`s that
/// represent the same conversation. Consecutive assistant items (text +
/// reasoning + function calls) collapse into a single `Message::Assistant`.
fn prompt_to_agentix_messages(prompt: &Prompt) -> Vec<Message> {
    let items = prompt.get_formatted_input();
    let mut out: Vec<Message> = Vec::new();
    let mut i = 0;
    while i < items.len() {
        match &items[i] {
            ResponseItem::Message { role, content, .. } if role == "user" => {
                out.push(Message::User(content_items_to_agentix(content)));
                i += 1;
            }
            ResponseItem::Message { role, .. }
                if role == "assistant" || role == "system" =>
            {
                let (msg, consumed) = collect_assistant_run(&items[i..]);
                out.push(msg);
                i += consumed;
            }
            ResponseItem::Reasoning { .. } | ResponseItem::FunctionCall { .. } => {
                let (msg, consumed) = collect_assistant_run(&items[i..]);
                out.push(msg);
                i += consumed;
            }
            ResponseItem::FunctionCallOutput { call_id, output } => {
                let body = output
                    .body
                    .to_text()
                    .unwrap_or_else(|| "(non-text tool output)".to_string());
                out.push(Message::ToolResult {
                    call_id: call_id.clone(),
                    content: vec![Content::text(body)],
                });
                i += 1;
            }
            // Custom tool calls / outputs, web search, tool search, local
            // shell calls — none of these have an agentix equivalent, so we
            // skip them silently rather than corrupt the message stream.
            _ => {
                i += 1;
            }
        }
    }
    out
}

/// Greedily collect a contiguous block of assistant-side items
/// (Message{role:"assistant"|"system"}, Reasoning, FunctionCall) into a
/// single `Message::Assistant`. Returns the message and the number of items
/// consumed.
fn collect_assistant_run(items: &[ResponseItem]) -> (Message, usize) {
    let mut text: Option<String> = None;
    let mut reasoning: Option<String> = None;
    let mut tool_calls: Vec<AgentixToolCall> = Vec::new();
    let mut consumed = 0;

    for item in items {
        match item {
            ResponseItem::Message { role, content, .. }
                if role == "assistant" || role == "system" =>
            {
                let extracted = extract_assistant_text(content);
                if !extracted.is_empty() {
                    text = Some(text.unwrap_or_default() + &extracted);
                }
            }
            ResponseItem::Reasoning {
                summary, content, ..
            } => {
                let r = extract_reasoning_text(summary, content.as_deref());
                if !r.is_empty() {
                    reasoning = Some(reasoning.unwrap_or_default() + &r);
                }
            }
            ResponseItem::FunctionCall {
                name,
                arguments,
                call_id,
                ..
            } => {
                tool_calls.push(AgentixToolCall {
                    id: call_id.clone(),
                    name: name.clone(),
                    arguments: arguments.clone(),
                });
            }
            _ => break,
        }
        consumed += 1;
    }

    let msg = Message::Assistant {
        content: text,
        reasoning,
        tool_calls,
        provider_data: None,
    };
    (msg, consumed)
}

fn extract_assistant_text(items: &[ContentItem]) -> String {
    let mut out = String::new();
    for item in items {
        if let ContentItem::OutputText { text } = item {
            out.push_str(text);
        }
    }
    out
}

fn extract_reasoning_text(
    summary: &[codex_protocol::models::ReasoningItemReasoningSummary],
    content: Option<&[codex_protocol::models::ReasoningItemContent]>,
) -> String {
    use codex_protocol::models::ReasoningItemContent;
    use codex_protocol::models::ReasoningItemReasoningSummary;

    let mut out = String::new();
    for s in summary {
        match s {
            ReasoningItemReasoningSummary::SummaryText { text } => {
                out.push_str(text);
                out.push('\n');
            }
        }
    }
    if let Some(c) = content {
        for c in c {
            match c {
                ReasoningItemContent::ReasoningText { text }
                | ReasoningItemContent::Text { text } => {
                    out.push_str(text);
                    out.push('\n');
                }
            }
        }
    }
    out
}

fn content_items_to_agentix(items: &[ContentItem]) -> Vec<Content> {
    items
        .iter()
        .filter_map(|item| match item {
            ContentItem::InputText { text } | ContentItem::OutputText { text } => {
                Some(Content::text(text.clone()))
            }
            ContentItem::InputImage { image_url, .. } => Some(Content::Image(ImageContent {
                data: ImageData::Url(image_url.clone()),
                mime_type: image_mime_from_url(image_url).to_string(),
            })),
        })
        .collect()
}

/// Best-effort mime-type lookup from a URL or data-uri suffix. Anthropic
/// requires this field even for URL-style images; other providers tolerate
/// any value.
fn image_mime_from_url(url: &str) -> &'static str {
    let lower = url.to_ascii_lowercase();
    if lower.contains(".png") {
        "image/png"
    } else if lower.contains(".jpg") || lower.contains(".jpeg") {
        "image/jpeg"
    } else if lower.contains(".gif") {
        "image/gif"
    } else if lower.contains(".webp") {
        "image/webp"
    } else {
        "image/png"
    }
}

/// Translate Codex tool specs into agentix `ToolDefinition`s.
///
/// Only `Function` (and `Namespace`, flattened into its children) variants
/// are translatable. Freeform tools (apply_patch's Lark grammar), local
/// shell, image generation, web search, and tool_search all rely on
/// OpenAI-Responses-API features that agentix does not model — these are
/// dropped with a warning so the stream still works.
fn tools_to_agentix_definitions(tools: &[ToolSpec]) -> Vec<ToolDefinition> {
    let mut out = Vec::new();
    for tool in tools {
        match tool {
            ToolSpec::Function(t) => {
                out.push(ToolDefinition::function(FunctionDefinition {
                    name: t.name.clone(),
                    description: Some(t.description.clone()),
                    parameters: serde_json::to_value(&t.parameters).unwrap_or(serde_json::json!({})),
                    strict: Some(t.strict),
                }));
            }
            ToolSpec::Namespace(ns) => {
                for child in &ns.tools {
                    let codex_tools::ResponsesApiNamespaceTool::Function(t) = child;
                    out.push(ToolDefinition::function(FunctionDefinition {
                        name: format!("{}_{}", ns.name, t.name),
                        description: Some(t.description.clone()),
                        parameters: serde_json::to_value(&t.parameters)
                            .unwrap_or(serde_json::json!({})),
                        strict: Some(t.strict),
                    }));
                }
            }
            other => {
                warn!(
                    "agentix wire-api: dropping unsupported ToolSpec variant {:?}",
                    other.name()
                );
            }
        }
    }
    out
}

fn accumulate_usage(mut a: UsageStats, b: UsageStats) -> UsageStats {
    a += b;
    a
}

fn usage_stats_to_token_usage(u: UsageStats) -> TokenUsage {
    TokenUsage {
        input_tokens: u.prompt_tokens as i64,
        cached_input_tokens: u.cache_read_tokens as i64,
        output_tokens: u.completion_tokens as i64,
        reasoning_output_tokens: u.reasoning_tokens as i64,
        total_tokens: u.total_tokens as i64,
    }
}
