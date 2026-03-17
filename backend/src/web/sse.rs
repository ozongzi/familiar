use std::cmp;
use std::convert::Infallible;

use async_stream::stream;
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::{
        IntoResponse,
        sse::{Event, KeepAlive, Sse},
    },
};
use serde::Deserialize;
use serde_json::json;
use sqlx::Row;
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::errors::AppError;
use crate::web::{AppState, auth::AuthUser};

// ── Request body types ────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct SendMessageRequest {
    pub content: String,
}

#[derive(Deserialize)]
pub struct InterruptRequest {
    pub content: String,
}

#[derive(Deserialize)]
pub struct AnswerRequest {
    pub content: String,
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Returns true if an SSE event payload represents a terminal event
/// (generation is done and the stream can be closed).
fn is_terminal(payload: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(payload)
        .ok()
        .and_then(|v| v.get("type").and_then(|t| t.as_str()).map(str::to_string))
        .map(|t| matches!(t.as_str(), "done" | "aborted" | "error"))
        .unwrap_or(false)
}

/// Verify that the authenticated user owns the given conversation.
async fn verify_conversation_owner(
    state: &AppState,
    conversation_id: Uuid,
    user_id: Uuid,
) -> Result<(), AppError> {
    let row = sqlx::query("SELECT user_id FROM conversations WHERE id = $1")
        .bind(conversation_id)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| {
            tracing::error!("conversation owner lookup: {e}");
            AppError::internal("数据库错误")
        })?
        .ok_or_else(|| AppError::not_found("对话不存在"))?;

    let owner: Uuid = row
        .try_get("user_id")
        .map_err(|_| AppError::internal("数据库错误"))?;
    if owner != user_id {
        return Err(AppError::forbidden("无权访问该对话"));
    }
    Ok(())
}

/// Resolve a stream token and verify ownership in one step.
/// Returns `(conversation_id, user_id)`.
fn resolve_and_check(
    state: &AppState,
    stream_id: Uuid,
    caller_user_id: Uuid,
) -> Result<(Uuid, Uuid), AppError> {
    let (conv_id, owner_id) = state
        .resolve_stream(stream_id)
        .ok_or_else(|| AppError::not_found("流不存在"))?;

    if owner_id != caller_user_id {
        return Err(AppError::forbidden("无权访问该流"));
    }
    Ok((conv_id, owner_id))
}

// ── POST /api/conversations/{id}/messages ─────────────────────────────────────

pub async fn send_message_handler(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(conversation_id): Path<Uuid>,
    Json(body): Json<SendMessageRequest>,
) -> Result<impl IntoResponse, AppError> {
    let content = body.content.trim().to_string();
    if content.is_empty() {
        return Err(AppError::bad_request("消息内容不能为空"));
    }

    // Verify the conversation exists and belongs to this user.
    verify_conversation_owner(&state, conversation_id, auth.user_id).await?;

    // Ensure ChatEntry exists and check if generation is in progress.
    let (_rx, _log, already_generating) = state.attach(conversation_id).await;

    // Persist the user message.
    {
        use ds_api::raw::request::message::{Message as AgentMessage, Role};
        let msg = AgentMessage::new(Role::User, &content);
        state.persist_message(conversation_id, &msg);
    }

    if already_generating {
        // Agent busy — inject via interrupt channel so it is processed
        // by the running turn. The client subscribes to the stream as usual
        // and will see the continuation events.
        tracing::info!(
            conversation = %conversation_id,
            "agent busy, injecting message via interrupt"
        );
        state.send_interrupt(conversation_id, content);

        let stream_id = state.create_stream(conversation_id, auth.user_id);
        return Ok((
            StatusCode::ACCEPTED,
            Json(json!({ "stream_id": stream_id })),
        ));
    }

    // Start a new background generation task.
    let started = state.start_generation(conversation_id, content).await;
    if !started {
        return Err(AppError::internal("无法启动生成任务"));
    }

    let stream_id = state.create_stream(conversation_id, auth.user_id);
    Ok((
        StatusCode::ACCEPTED,
        Json(json!({ "stream_id": stream_id })),
    ))
}

// ── GET /api/stream/{stream_id} ───────────────────────────────────────────────

pub async fn sse_handler(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(stream_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    let (conversation_id, _owner_id) = resolve_and_check(&state, stream_id, auth.user_id)?;

    // Get a broadcast receiver + current event log snapshot.
    let (mut live_rx, event_log, _generating) = state.attach(conversation_id).await;

    // Check if generation already finished before this SSE connection arrived.
    let already_done = event_log
        .last()
        .map(|ev| is_terminal(&ev.payload))
        .unwrap_or(false);

    let s = stream! {
        // Use sequence numbers for deduplication: every WsEvent has a globally
        // unique monotonically-increasing seq assigned at emit() time.
        // We track the highest seq we have already yielded; anything ≤ that
        // seq arriving from live_rx is a duplicate and gets dropped.
        // This replaces the old Arc-pointer trick which failed for spawn events
        // (each relay creates a fresh Arc from a String, so pointers diverge).
        let mut last_sent_seq: Option<u64> = None;

        // ── Replay the event log ─────────────────────────────────────────────
        for ev in &event_log {
            last_sent_seq = Some(match last_sent_seq {
                Some(s) => cmp::max(s, ev.seq),
                None => ev.seq,
            });
            yield Ok::<Event, Infallible>(Event::default().data(ev.payload.clone()));
        }

        if already_done {
            // Generation already finished — nothing more to stream.
            return;
        }

        // ── Relay live broadcast events ───────────────────────────────────────
        loop {
            match live_rx.recv().await {
                Ok(ev) => {
                    // Skip events we already sent during replay.
                    if let Some(last) = last_sent_seq {
                        if ev.seq <= last {
                            continue;
                        }
                    }
                    last_sent_seq = Some(ev.seq);
                    let terminal = is_terminal(&ev.payload);
                    yield Ok(Event::default().data(ev.payload.clone()));
                    if terminal {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(
                        conversation = %conversation_id,
                        "SSE broadcast lagged by {n}, replaying event log from seq {:?}",
                        last_sent_seq,
                    );
                    // Re-attach to get the full log and a fresh receiver.
                    let (new_rx, new_log, _) = state.attach(conversation_id).await;
                    live_rx = new_rx;

                    // Replay only events we haven't sent yet (seq > last_sent_seq).
                    let new_done = new_log
                        .last()
                        .map(|ev| is_terminal(&ev.payload))
                        .unwrap_or(false);

                    for ev in &new_log {
                        if let Some(last) = last_sent_seq {
                            if ev.seq <= last {
                                continue;
                            }
                        }
                        last_sent_seq = Some(ev.seq);
                        yield Ok(Event::default().data(ev.payload.clone()));
                    }

                    if new_done {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Closed) => {
                    // The sender was dropped — generation task ended.
                    break;
                }
            }
        }
    };

    Ok(Sse::new(s).keep_alive(KeepAlive::default()))
}

// ── POST /api/stream/{stream_id}/abort ───────────────────────────────────────

pub async fn stream_abort_handler(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(stream_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    let (conversation_id, _) = resolve_and_check(&state, stream_id, auth.user_id)?;
    state.abort_generation(conversation_id);
    Ok(StatusCode::OK)
}

// ── POST /api/stream/{stream_id}/interrupt ────────────────────────────────────

pub async fn stream_interrupt_handler(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(stream_id): Path<Uuid>,
    Json(body): Json<InterruptRequest>,
) -> Result<impl IntoResponse, AppError> {
    let content = body.content.trim().to_string();
    if content.is_empty() {
        return Err(AppError::bad_request("中断内容不能为空"));
    }

    let (conversation_id, _) = resolve_and_check(&state, stream_id, auth.user_id)?;

    state.send_interrupt(conversation_id, content.clone());

    // Persist the interrupt as a user message.
    {
        use ds_api::raw::request::message::{Message as AgentMessage, Role};
        let msg = AgentMessage::new(Role::User, &content);
        state.persist_message(conversation_id, &msg);
    }

    Ok(StatusCode::OK)
}

// ── POST /api/stream/{stream_id}/answer ──────────────────────────────────────

pub async fn stream_answer_handler(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(stream_id): Path<Uuid>,
    Json(body): Json<AnswerRequest>,
) -> Result<impl IntoResponse, AppError> {
    let content = body.content.trim().to_string();
    if content.is_empty() {
        return Err(AppError::bad_request("回答内容不能为空"));
    }

    let (conversation_id, _) = resolve_and_check(&state, stream_id, auth.user_id)?;

    state.deliver_answer(conversation_id, content).await;

    Ok(StatusCode::OK)
}
