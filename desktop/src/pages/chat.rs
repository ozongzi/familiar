use dioxus::prelude::*;
use tokio::sync::mpsc;

use crate::components::input::Composer;
use crate::components::message::TurnView;
use crate::llm::{Event, StopReason, anthropic};
use crate::state::AppState;
use crate::storage::conversation::{Block, Conversation, Role, Turn};
use crate::tools;

#[component]
pub fn ChatPage() -> Element {
    let state = use_context::<AppState>();
    let conversation = state.conversation;
    let generating = state.generating;
    let error = state.error;

    let on_send = move |text: String| {
        spawn(async move {
            run_agent_loop(text).await;
        });
    };

    rsx! {
        div { class: "chat",
            if let Some(err) = error.read().clone() {
                div { class: "error-banner",
                    span { "{err}" }
                    button { onclick: move |_| {
                        let mut e = error;
                        e.set(None);
                    }, "✕" }
                }
            }
            div { class: "messages",
                if let Some(c) = conversation.read().as_ref() {
                    div { class: "conv-meta", "{c.meta.title} · {c.meta.model}" }
                    for (i, t) in c.turns.iter().enumerate() {
                        TurnView { key: "{i}", turn: t.clone() }
                    }
                } else {
                    div { class: "empty",
                        h2 { "Familiar" }
                        p { "本地客户端 — 你的对话、记忆、技能都是本地 markdown 文件。" }
                        p { class: "hint", "点击左上 \"+ 新对话\" 开始。" }
                    }
                }
            }
            Composer {
                disabled: generating() || conversation.read().is_none(),
                on_send: on_send,
            }
        }
    }
}

/// Drive one user turn end-to-end: append user message, stream assistant
/// response, run any tool_uses, loop until model returns end_turn.
async fn run_agent_loop(user_text: String) {
    let mut state = use_context::<AppState>();
    let cfg = state.config.read().clone();

    if cfg.anthropic_api_key.is_empty() {
        state.error.set(Some("未设置 Anthropic API key — 打开设置填一个".into()));
        return;
    }

    // Snapshot the current conversation; we mutate this local copy and write
    // it back to the signal as we go.
    let mut conv = match state.conversation.read().clone() {
        Some(c) => c,
        None => return,
    };

    // Append user turn.
    conv.turns.push(Turn {
        role: Role::User,
        blocks: vec![Block::Text { text: user_text.clone() }],
    });
    if conv.turns.len() == 1 {
        // First user message becomes the title.
        conv.meta.title = derive_title(&user_text);
    }
    conv.meta.updated_at = chrono::Utc::now();
    let _ = conv.save();
    state.conversation.set(Some(conv.clone()));
    state.refresh_list();

    state.generating.set(true);
    state.error.set(None);

    let conv_id = conv.meta.id.clone();
    let tools_spec = tools::tool_specs();

    // Outer loop: keep streaming until the model says end_turn (or errors).
    loop {
        // Build a placeholder assistant turn that we'll fill incrementally.
        conv.turns.push(Turn { role: Role::Assistant, blocks: Vec::new() });
        state.conversation.set(Some(conv.clone()));

        let (tx, mut rx) = mpsc::unbounded_channel::<Event>();
        let req = anthropic::StreamRequest {
            api_key: cfg.anthropic_api_key.clone(),
            model: cfg.model.clone(),
            max_tokens: cfg.max_tokens,
            system: cfg.system_prompt.clone(),
            conversation: conv.clone(),
            tools: tools_spec.clone(),
        };

        let stream_handle = tokio::spawn(async move {
            if let Err(e) = anthropic::stream(req, tx.clone()).await {
                let _ = tx.send(Event::Error(format!("{e}")));
            }
        });

        // State for the in-flight assistant turn.
        let mut text_buf = String::new();
        let mut pending_tool: Option<(String, String, String)> = None; // id, name, json_buf
        let mut stop_reason: Option<StopReason> = None;
        let mut had_error = false;

        while let Some(evt) = rx.recv().await {
            match evt {
                Event::TextDelta(t) => {
                    text_buf.push_str(&t);
                    update_last_assistant(&mut conv, &text_buf, pending_tool.as_ref());
                    state.conversation.set(Some(conv.clone()));
                }
                Event::ToolUseStart { id, name } => {
                    // Flush text buffer to a Text block first.
                    if !text_buf.trim().is_empty() {
                        commit_text(&mut conv, &text_buf);
                        text_buf.clear();
                    }
                    pending_tool = Some((id, name, String::new()));
                    update_last_assistant(&mut conv, &text_buf, pending_tool.as_ref());
                    state.conversation.set(Some(conv.clone()));
                }
                Event::ToolUseInputDelta(t) => {
                    if let Some((_, _, buf)) = pending_tool.as_mut() {
                        buf.push_str(&t);
                    }
                    update_last_assistant(&mut conv, &text_buf, pending_tool.as_ref());
                    state.conversation.set(Some(conv.clone()));
                }
                Event::Done(reason) => {
                    stop_reason = Some(reason);
                    // Channel may still have late events; let it drain naturally
                    // by letting stream_handle finish, but break here since
                    // message_stop typically comes right after.
                }
                Event::Error(e) => {
                    state.error.set(Some(e));
                    had_error = true;
                    break;
                }
            }
        }

        let _ = stream_handle.await;

        // Commit any trailing text.
        if !text_buf.trim().is_empty() {
            commit_text(&mut conv, &text_buf);
        }
        // Commit any pending tool_use with parsed input.
        let mut tool_to_run: Option<(String, String, serde_json::Value)> = None;
        if let Some((id, name, buf)) = pending_tool {
            let input: serde_json::Value =
                serde_json::from_str(&buf).unwrap_or(serde_json::json!({}));
            commit_tool_use(&mut conv, &id, &name, &input);
            tool_to_run = Some((id, name, input));
        }

        conv.meta.updated_at = chrono::Utc::now();
        let _ = conv.save();
        state.conversation.set(Some(conv.clone()));
        state.refresh_list();

        if had_error {
            break;
        }

        match stop_reason {
            Some(StopReason::ToolUse) => {
                let Some((id, name, input)) = tool_to_run else {
                    break;
                };
                let outcome = tools::dispatch(&conv_id, &name, &input).await;
                // Anthropic requires tool_result to be in the *next* user turn.
                conv.turns.push(Turn {
                    role: Role::User,
                    blocks: vec![Block::ToolResult {
                        id,
                        content: outcome.content,
                        is_error: outcome.is_error,
                    }],
                });
                conv.meta.updated_at = chrono::Utc::now();
                let _ = conv.save();
                state.conversation.set(Some(conv.clone()));
                continue; // loop again — model needs to see the result
            }
            _ => break,
        }
    }

    state.generating.set(false);
}

fn derive_title(text: &str) -> String {
    let trimmed = text.trim();
    let first_line = trimmed.lines().next().unwrap_or(trimmed);
    let mut s: String = first_line.chars().take(40).collect();
    if first_line.chars().count() > 40 {
        s.push('…');
    }
    if s.is_empty() { "新对话".into() } else { s }
}

fn update_last_assistant(
    conv: &mut Conversation,
    text_buf: &str,
    pending_tool: Option<&(String, String, String)>,
) {
    let Some(turn) = conv.turns.last_mut() else { return };
    if turn.role != Role::Assistant {
        return;
    }
    // Rebuild blocks as committed_blocks + (live text if non-empty) + (live tool if pending).
    // Committed blocks are everything already present; we never strip them.
    // Live preview blocks are added as transient last entries.
    // To keep this simple, we strip any trailing "preview" blocks (marked by being
    // either an empty Text or by pending_tool match) before re-adding.
    while matches!(turn.blocks.last(), Some(Block::Text { text }) if text == "__preview__")
        || matches!(turn.blocks.last(), Some(Block::ToolUse { id, .. }) if pending_tool.as_ref().map(|t| &t.0) == Some(id))
    {
        turn.blocks.pop();
    }
    if !text_buf.is_empty() {
        // Live text preview — committed on next non-text event.
        // We mark it by content equality, not a flag, so we use a non-printing
        // sentinel? Simpler: the last text block in committed state can't be
        // identical to live buffer because commit happens separately. So we
        // just append the live text as a Text block; when next delta arrives,
        // we rebuild from text_buf.
        // To avoid duplicate growth, strip the previous live preview first.
        if let Some(Block::Text { text }) = turn.blocks.last() {
            // Heuristic: if the trailing block is a prefix of text_buf, replace it.
            if text_buf.starts_with(text.as_str()) || text.starts_with(text_buf) {
                turn.blocks.pop();
            }
        }
        turn.blocks.push(Block::Text { text: text_buf.to_string() });
    }
    if let Some((id, name, buf)) = pending_tool {
        let input: serde_json::Value =
            serde_json::from_str(buf).unwrap_or_else(|_| serde_json::json!({"_partial": buf}));
        turn.blocks.push(Block::ToolUse {
            id: id.clone(),
            name: name.clone(),
            input,
        });
    }
}

fn commit_text(conv: &mut Conversation, text: &str) {
    let Some(turn) = conv.turns.last_mut() else { return };
    if turn.role != Role::Assistant {
        return;
    }
    if let Some(Block::Text { text: prev }) = turn.blocks.last() {
        if prev == text || text.starts_with(prev.as_str()) {
            turn.blocks.pop();
        }
    }
    turn.blocks.push(Block::Text { text: text.trim_end().to_string() });
}

fn commit_tool_use(
    conv: &mut Conversation,
    id: &str,
    name: &str,
    input: &serde_json::Value,
) {
    let Some(turn) = conv.turns.last_mut() else { return };
    if turn.role != Role::Assistant {
        return;
    }
    // Replace any preview tool_use block with the same id.
    if let Some(Block::ToolUse { id: prev_id, .. }) = turn.blocks.last() {
        if prev_id == id {
            turn.blocks.pop();
        }
    }
    turn.blocks.push(Block::ToolUse {
        id: id.to_string(),
        name: name.to_string(),
        input: input.clone(),
    });
}
