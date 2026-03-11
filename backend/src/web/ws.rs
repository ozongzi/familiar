use std::sync::Arc;

use axum::{
    extract::{
        Path, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    response::Response,
};
use futures::{SinkExt, StreamExt};
use serde_json::json;
use sqlx::Row;
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::errors::AppError;
use crate::state::WsEvent;
use crate::web::AppState;

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Path(conversation_id): Path<Uuid>,
) -> Result<Response, AppError> {
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM conversations WHERE id = $1)")
            .bind(conversation_id)
            .fetch_one(&state.pool)
            .await
            .map_err(|e| {
                tracing::error!("ws conversation lookup: {e}");
                AppError::internal("数据库错误")
            })?;

    if !exists {
        return Err(AppError::not_found("对话不存在"));
    }

    Ok(ws.on_upgrade(move |socket| handle_socket(socket, state, conversation_id)))
}

async fn handle_socket(socket: WebSocket, state: AppState, conversation_id: Uuid) {
    let (mut sender, mut receiver) = socket.split();

    // ── Auth handshake ────────────────────────────────────────────────────────
    // First message: { "token": "<bearer>" }
    let _user_id = match receiver.next().await {
        Some(Ok(Message::Text(txt))) => {
            let v: serde_json::Value = match serde_json::from_str(&txt) {
                Ok(v) => v,
                Err(_) => {
                    let _ = sender
                        .send(Message::Text(
                            json!({"type":"error","message":"invalid auth message"})
                                .to_string()
                                .into(),
                        ))
                        .await;
                    return;
                }
            };
            let token = match v.get("token").and_then(|t| t.as_str()) {
                Some(t) => t.to_string(),
                None => {
                    let _ = sender
                        .send(Message::Text(
                            json!({"type":"error","message":"missing token"})
                                .to_string()
                                .into(),
                        ))
                        .await;
                    return;
                }
            };

            match sqlx::query(
                r#"
                SELECT c.user_id
                FROM sessions s
                JOIN conversations c ON c.user_id = s.user_id
                WHERE s.token = $1 AND c.id = $2
                "#,
            )
            .bind(token)
            .bind(conversation_id)
            .fetch_optional(&state.pool)
            .await
            {
                Ok(Some(row)) => row.try_get::<Uuid, _>("user_id").unwrap_or(Uuid::nil()),
                Ok(None) => {
                    let _ = sender
                        .send(Message::Text(
                            json!({"type":"error","message":"unauthorized"})
                                .to_string()
                                .into(),
                        ))
                        .await;
                    return;
                }
                Err(e) => {
                    tracing::error!("ws auth query: {e}");
                    let _ = sender
                        .send(Message::Text(
                            json!({"type":"error","message":"db error"})
                                .to_string()
                                .into(),
                        ))
                        .await;
                    return;
                }
            }
        }
        _ => return,
    };

    // ── Second message: either a new user turn or a "reattach" ───────────────
    // { "content": "..." }  → start a new generation turn
    // { "type": "reattach" } → just subscribe to ongoing/completed generation
    let second: serde_json::Value = match receiver.next().await {
        Some(Ok(Message::Text(txt))) => match serde_json::from_str(&txt) {
            Ok(v) => v,
            Err(_) => {
                let _ = sender
                    .send(Message::Text(
                        json!({"type":"error","message":"invalid message"})
                            .to_string()
                            .into(),
                    ))
                    .await;
                return;
            }
        },
        _ => return,
    };

    // ── Ensure ChatEntry exists and get a broadcast receiver + event log ──────
    let (mut live_rx, event_log, already_generating) = state.attach(conversation_id).await;

    let is_reattach = second
        .get("type")
        .and_then(|t| t.as_str())
        .map(|t| t == "reattach")
        .unwrap_or(false);

    if is_reattach {
        // Client reconnected mid-generation or after — just replay + relay.
        tracing::info!(conversation = %conversation_id, "client reattached");
    } else {
        // New user message.
        let user_text = match second
            .get("content")
            .and_then(|c| c.as_str())
            .map(|s| s.to_string())
        {
            Some(t) if !t.trim().is_empty() => t,
            _ => {
                let _ = sender
                    .send(Message::Text(
                        json!({"type":"error","message":"empty content"})
                            .to_string()
                            .into(),
                    ))
                    .await;
                return;
            }
        };

        // Persist user message.
        {
            use ds_api::raw::request::message::{Message as AgentMessage, Role};
            let msg = AgentMessage::new(Role::User, &user_text);
            state.persist_message(conversation_id, &msg);
        }

        if already_generating {
            // Agent is busy — inject via interrupt channel instead.
            tracing::info!(conversation = %conversation_id, "agent busy, injecting via interrupt");
            state.send_interrupt(conversation_id, user_text.clone());
            // Echo the injected message back to this client.
            let _ = sender
                .send(Message::Text(
                    json!({"type": "user_interrupt", "content": user_text})
                        .to_string()
                        .into(),
                ))
                .await;
        } else {
            // Start a new background generation task.
            let started = state.start_generation(conversation_id, user_text).await;
            if !started {
                let _ = sender
                    .send(Message::Text(
                        json!({"type":"error","message":"failed to start generation"})
                            .to_string()
                            .into(),
                    ))
                    .await;
                return;
            }
            // Re-subscribe so we get a fresh receiver aligned with the new turn's log.
            // (The previous rx was created before start_generation cleared the log.)
            let (new_rx, new_log, _) = state.attach(conversation_id).await;
            live_rx = new_rx;
            // new_log should be empty or contain only events emitted so far in
            // this brand-new turn — replay it below.
            let _ = replay_log(&mut sender, &new_log).await;
            // Skip the second replay below.
            relay_live(&mut sender, &mut receiver, live_rx, state, conversation_id).await;
            return;
        }
    }

    // ── Replay the event log to catch the client up ───────────────────────────
    if replay_log(&mut sender, &event_log).await.is_err() {
        return;
    }

    // If the log already ends with "done" or "aborted", the generation is over —
    // no need to subscribe to live events.
    let last_type = event_log.last().and_then(|ev| {
        serde_json::from_str::<serde_json::Value>(&ev.payload)
            .ok()
            .and_then(|v| {
                v.get("type")
                    .and_then(|t| t.as_str())
                    .map(|s| s.to_string())
            })
    });

    if matches!(
        last_type.as_deref(),
        Some("done") | Some("aborted") | Some("error")
    ) {
        return;
    }

    // ── Relay live events until done ──────────────────────────────────────────
    relay_live(&mut sender, &mut receiver, live_rx, state, conversation_id).await;
}

/// Send every event in the log to the client.
async fn replay_log(
    sender: &mut futures::stream::SplitSink<WebSocket, Message>,
    log: &[Arc<WsEvent>],
) -> Result<(), ()> {
    for ev in log {
        if sender
            .send(Message::Text(ev.payload.clone().into()))
            .await
            .is_err()
        {
            return Err(());
        }
    }
    Ok(())
}

/// Forward live broadcast events to the client, and handle incoming client
/// control messages (abort / interrupt) until the generation finishes.
async fn relay_live(
    sender: &mut futures::stream::SplitSink<WebSocket, Message>,
    receiver: &mut futures::stream::SplitStream<WebSocket>,
    mut live_rx: broadcast::Receiver<Arc<WsEvent>>,
    state: AppState,
    conversation_id: Uuid,
) {
    loop {
        tokio::select! {
            // ── Incoming broadcast event from the generation task ─────────
            result = live_rx.recv() => {
                match result {
                    Ok(ev) => {
                        // Forward to client.
                        if sender
                            .send(Message::Text(ev.payload.clone().into()))
                            .await
                            .is_err()
                        {
                            // Client disconnected — generation keeps running in background.
                            return;
                        }

                        // Stop relaying when the generation is finished.
                        let ev_type = serde_json::from_str::<serde_json::Value>(&ev.payload)
                            .ok()
                            .and_then(|v| {
                                v.get("type").and_then(|t| t.as_str()).map(|s| s.to_string())
                            });
                        if matches!(ev_type.as_deref(), Some("done") | Some("aborted") | Some("error")) {
                            return;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        // We missed some events — re-attach to get the full log.
                        tracing::warn!(
                            conversation = %conversation_id,
                            "broadcast lagged by {n}, resyncing event log"
                        );
                        let (new_rx, log, _) = state.attach(conversation_id).await;
                        live_rx = new_rx;
                        // Replay the full log from the beginning (client will see
                        // duplicates for events it already received — acceptable).
                        if replay_log(sender, &log).await.is_err() {
                            return;
                        }
                        // Check if already done.
                        let last_type = log.last().and_then(|ev| {
                            serde_json::from_str::<serde_json::Value>(&ev.payload)
                                .ok()
                                .and_then(|v| {
                                    v.get("type")
                                        .and_then(|t| t.as_str())
                                        .map(|s| s.to_string())
                                })
                        });
                        if matches!(last_type.as_deref(), Some("done") | Some("aborted") | Some("error")) {
                            return;
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        // Sender dropped — generation task ended.
                        return;
                    }
                }
            }

            // ── Incoming client control message ───────────────────────────
            ws_msg = receiver.next() => {
                let Some(Ok(Message::Text(txt))) = ws_msg else {
                    // Socket closed — generation keeps running.
                    return;
                };

                let v: serde_json::Value = match serde_json::from_str(&txt) {
                    Ok(v) => v,
                    Err(_) => continue,
                };

                match v.get("type").and_then(|t| t.as_str()) {
                    Some("abort") => {
                        tracing::info!(conversation = %conversation_id, "client requested abort");
                        state.abort_generation(conversation_id);
                        // Don't close — wait for the "aborted" event from the task.
                    }
                    Some("answer") => {
                        // Deliver the user's reply to a waiting ask_user spell.
                        if let Some(content) = v.get("content").and_then(|c| c.as_str())
                            && !content.trim().is_empty() {
                                state.deliver_answer(conversation_id, content.to_string()).await;
                            }
                    }
                    Some("interrupt") => {
                        if let Some(content) = v.get("content").and_then(|c| c.as_str())
                            && !content.trim().is_empty() {
                                tracing::info!(
                                    conversation = %conversation_id,
                                    "client interrupt: {content}"
                                );
                                state.send_interrupt(conversation_id, content.to_string());
                                // Persist the interrupt message.
                                use ds_api::raw::request::message::{Message as AgentMessage, Role};
                                let msg = AgentMessage::new(Role::User, content);
                                state.persist_message(conversation_id, &msg);
                                // Echo back to the client.
                                let _ = sender
                                    .send(Message::Text(
                                        json!({
                                            "type": "user_interrupt",
                                            "content": content,
                                        })
                                        .to_string()
                                        .into(),
                                    ))
                                    .await;
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}
