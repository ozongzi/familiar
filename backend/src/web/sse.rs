use std::convert::Infallible;
use std::time::Duration;

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
use uuid::Uuid;

use crate::errors::AppError;
use crate::web::{AppState, auth::AuthUser};

// ── Request body types ────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct SendMessageRequest {
    pub content: String,
    #[serde(default)]
    pub images: Vec<String>,
}

#[derive(Deserialize)]
pub struct InterruptRequest {
    pub content: String,
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn is_terminal(payload: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(payload)
        .ok()
        .and_then(|v| v.get("type").and_then(|t| t.as_str()).map(str::to_string))
        .map(|t| matches!(t.as_str(), "done" | "aborted" | "error"))
        .unwrap_or(false)
}

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

/// Resolve a job_id (used as stream_id) and verify ownership.
/// Returns `(conversation_id, user_id)`.
async fn resolve_job(
    state: &AppState,
    job_id: Uuid,
    caller_user_id: Uuid,
) -> Result<(Uuid, Uuid), AppError> {
    let row = sqlx::query("SELECT conversation_id, user_id FROM generation_jobs WHERE id = $1")
        .bind(job_id)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| {
            tracing::error!("job lookup: {e}");
            AppError::internal("数据库错误")
        })?
        .ok_or_else(|| AppError::not_found("流不存在"))?;

    let conv_id: Uuid = row.try_get("conversation_id").map_err(|_| AppError::internal("数据库错误"))?;
    let owner_id: Uuid = row.try_get("user_id").map_err(|_| AppError::internal("数据库错误"))?;

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
    let images = body.images;

    if content.is_empty() && images.is_empty() {
        return Err(AppError::bad_request("消息内容不能为空"));
    }

    verify_conversation_owner(&state, conversation_id, auth.user_id).await?;

    // Build multimodal parts.
    let user_parts: Vec<agentix::UserContent> = {
        use agentix::{ImageContent, ImageData, UserContent};
        let mut parts: Vec<UserContent> = images
            .iter()
            .map(|data_url| {
                let (mime, b64) = if let Some(rest) = data_url.strip_prefix("data:") {
                    if let Some(idx) = rest.find(";base64,") {
                        (&rest[..idx], &rest[idx + 8..])
                    } else {
                        ("image/jpeg", data_url.as_str())
                    }
                } else {
                    ("image/jpeg", data_url.as_str())
                };
                UserContent::Image(ImageContent {
                    data: ImageData::Base64(b64.to_string()),
                    mime_type: mime.to_string(),
                })
            })
            .collect();
        if !content.is_empty() {
            parts.push(UserContent::Text { text: content.clone() });
        }
        parts
    };

    // Persist the user message synchronously before starting generation.
    // Must be awaited: if we fire-and-forget, the worker's append_streaming()
    // can advance active_message_id first, causing the user message to be
    // inserted as a child of the streaming row (corrupted tree) and excluded
    // from the new worker's restore() call.
    let user_message_id = {
        use agentix::Message;
        let msg = Message::User(user_parts);
        state.persist_message_async(conversation_id, msg).await
    };

    // If there's already a running job, abort it first (interrupt semantics).
    // Then start a new generation job.
    let job_id = state
        .start_generation(conversation_id, auth.user_id)
        .await
        .map_err(|e| AppError::internal(&e.to_string()))?;

    // job_id doubles as stream_id
    Ok((
        StatusCode::ACCEPTED,
        Json(json!({ "stream_id": job_id, "user_message_id": user_message_id })),
    ))
}

// ── GET /api/stream/{stream_id} ───────────────────────────────────────────────

pub async fn sse_handler(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(stream_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    let job_id = stream_id;
    let (_conv_id, _owner_id) = resolve_job(&state, job_id, auth.user_id).await?;

    let pool = state.pool.clone();

    let s = stream! {
        // ── 1. Partial sync: send current streaming content if the job is active ─
        // This replaces replaying all token events on reconnect.  The client
        // replaces its streaming bubble content with the synced value.
        let partial_row = sqlx::query(
            "SELECT content, reasoning FROM messages WHERE job_id = $1 AND streaming = true LIMIT 1"
        )
        .bind(job_id)
        .fetch_optional(&pool)
        .await
        .unwrap_or(None);

        if let Some(row) = partial_row {
            let content: String = row.try_get("content").unwrap_or_default();
            let reasoning: String = row.try_get("reasoning").unwrap_or_default();
            if !content.is_empty() || !reasoning.is_empty() {
                yield Ok::<Event, Infallible>(Event::default().data(
                    json!({"type": "partial_sync", "content": content, "reasoning": reasoning}).to_string()
                ));
            }
        }

        // ── 2. Replay existing non-token events from DB ───────────────────────
        // Token events are covered by partial_sync; only replay structural events
        // (tool_call, tool_result, usage, done, aborted, error).
        let existing: Vec<(i64, String)> = sqlx::query_as(
            "SELECT id, payload FROM generation_events WHERE job_id = $1 ORDER BY id ASC"
        )
        .bind(job_id)
        .fetch_all(&pool)
        .await
        .unwrap_or_default();

        let mut last_id: i64 = 0;
        let mut done = false;

        for (id, payload) in &existing {
            last_id = *id;
            if is_terminal(payload) {
                done = true;
            }
            // Skip token events — already covered by partial_sync.
            let event_type = serde_json::from_str::<serde_json::Value>(payload)
                .ok()
                .and_then(|v| v.get("type").and_then(|t| t.as_str()).map(str::to_string));
            if matches!(event_type.as_deref(), Some("token") | Some("reasoning_token")) {
                continue;
            }
            yield Ok::<Event, Infallible>(Event::default().data(payload.clone()));
        }

        if done {
            return;
        }

        // ── 3. Subscribe to LISTEN/NOTIFY for live events ─────────────────────
        // We use a dedicated connection for LISTEN.
        let mut listener = match sqlx::postgres::PgListener::connect_with(&pool).await {
            Ok(l) => l,
            Err(e) => {
                tracing::error!("PgListener connect failed: {e}");
                yield Ok(Event::default().data(json!({"type": "error", "message": "SSE listener failed"}).to_string()));
                return;
            }
        };
        if let Err(e) = listener.listen("generation_events").await {
            tracing::error!("LISTEN failed: {e}");
            yield Ok(Event::default().data(json!({"type": "error", "message": "SSE listener failed"}).to_string()));
            return;
        }

        // Also check for any events we missed between the initial SELECT and LISTEN.
        let gap: Vec<(i64, String)> = sqlx::query_as(
            "SELECT id, payload FROM generation_events WHERE job_id = $1 AND id > $2 ORDER BY id ASC"
        )
        .bind(job_id)
        .bind(last_id)
        .fetch_all(&pool)
        .await
        .unwrap_or_default();

        for (id, payload) in &gap {
            last_id = *id;
            if is_terminal(payload) {
                done = true;
            }
            yield Ok(Event::default().data(payload.clone()));
        }

        if done {
            return;
        }

        // ── 3. Stream live events via NOTIFY ──────────────────────────────────
        loop {
            // Wait for notification with timeout (heartbeat / stale detection).
            match tokio::time::timeout(Duration::from_secs(30), listener.recv()).await {
                Ok(Ok(notification)) => {
                    let payload_str = notification.payload();

                    // Fast path: inline token payload "I:{job_id}:{json}"
                    if let Some(rest) = payload_str.strip_prefix("I:") {
                        // rest = "{job_id}:{json}"  — UUID is 36 chars + ':'
                        if rest.len() > 37 {
                            let (job_str, event_json) = rest.split_at(36);
                            let event_json = &event_json[1..]; // skip ':'
                            let notif_job: Option<Uuid> = job_str.parse().ok();
                            if notif_job == Some(job_id) && !event_json.is_empty() {
                                yield Ok(Event::default().data(event_json.to_string()));
                            }
                        }
                        continue;
                    }

                    // Reliable path: "job_id:event_id" — fetch from DB.
                    let parts: Vec<&str> = payload_str.splitn(2, ':').collect();
                    if parts.len() != 2 {
                        continue;
                    }
                    let notified_job: Option<Uuid> = parts[0].parse().ok();
                    let notified_event_id: Option<i64> = parts[1].parse().ok();

                    // Skip notifications for other jobs.
                    if notified_job != Some(job_id) {
                        continue;
                    }
                    // Skip events we already sent.
                    if let Some(eid) = notified_event_id
                        && eid <= last_id {
                            continue;
                    }

                    // Fetch new events from DB (batch — in case we missed some).
                    let new_events: Vec<(i64, String)> = sqlx::query_as(
                        "SELECT id, payload FROM generation_events WHERE job_id = $1 AND id > $2 ORDER BY id ASC"
                    )
                    .bind(job_id)
                    .bind(last_id)
                    .fetch_all(&pool)
                    .await
                    .unwrap_or_default();

                    for (id, payload) in &new_events {
                        last_id = *id;
                        let terminal = is_terminal(payload);
                        yield Ok(Event::default().data(payload.clone()));
                        if terminal {
                            return;
                        }
                    }
                }
                Ok(Err(e)) => {
                    tracing::error!("PgListener error: {e}");
                    break;
                }
                Err(_) => {
                    // Timeout — check if the job finished while we were waiting.
                    let status: Option<String> = sqlx::query_scalar(
                        "SELECT status FROM generation_jobs WHERE id = $1"
                    )
                    .bind(job_id)
                    .fetch_optional(&pool)
                    .await
                    .unwrap_or(None);

                    if matches!(status.as_deref(), Some("done" | "error" | "aborted")) {
                        // Drain any remaining events.
                        let remaining: Vec<(i64, String)> = sqlx::query_as(
                            "SELECT id, payload FROM generation_events WHERE job_id = $1 AND id > $2 ORDER BY id ASC"
                        )
                        .bind(job_id)
                        .bind(last_id)
                        .fetch_all(&pool)
                        .await
                        .unwrap_or_default();

                        for (_id, payload) in &remaining {
                            yield Ok(Event::default().data(payload.clone()));
                        }
                        break;
                    }
                    // Otherwise, keep waiting.
                }
            }
        }
    };

    Ok(Sse::new(s).keep_alive(KeepAlive::default()))
}

// ── POST /api/conversations/{id}/reattach ────────────────────────────────────

/// Find the latest job for a conversation and return its ID as stream_id.
/// If no active job, return null stream_id (frontend can handle this).
pub async fn reattach_handler(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(conversation_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    verify_conversation_owner(&state, conversation_id, auth.user_id).await?;

    // Only reattach to an active (pending/running) job.
    // Completed jobs don't need SSE replay — history is already loaded from the DB.
    let job_id: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM generation_jobs \
         WHERE conversation_id = $1 AND status IN ('pending', 'running') \
         ORDER BY created_at DESC LIMIT 1"
    )
    .bind(conversation_id)
    .fetch_optional(&state.pool)
    .await
    .unwrap_or(None);

    Ok((StatusCode::OK, Json(json!({ "stream_id": job_id }))))
}

// ── POST /api/conversations/{id}/branch ──────────────────────────────────────

#[derive(Deserialize)]
pub struct BranchRequest {
    pub message_id: i64,
}

pub async fn branch_handler(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(conversation_id): Path<Uuid>,
    Json(body): Json<BranchRequest>,
) -> Result<impl IntoResponse, AppError> {
    verify_conversation_owner(&state, conversation_id, auth.user_id).await?;

    let parent_id: Option<i64> = sqlx::query_scalar(
        "SELECT parent_id FROM messages WHERE id = $1"
    )
    .bind(body.message_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| AppError::internal(&e.to_string()))?
    .flatten();

    // When the edited message is the root (parent_id IS NULL), reset
    // active_message_id to NULL so the next insert becomes a new root node.
    // Otherwise point at the parent so the new message threads correctly.
    match parent_id {
        Some(pid) => {
            state.db.branch(conversation_id, pid).await
                .map_err(|e| AppError::internal(&e.to_string()))?;
        }
        None => {
            sqlx::query(
                "UPDATE conversations SET active_message_id = NULL WHERE id = $1"
            )
            .bind(conversation_id)
            .execute(&state.pool)
            .await
            .map_err(|e| AppError::internal(&e.to_string()))?;
        }
    }

    Ok(StatusCode::OK)
}

// ── POST /api/stream/{stream_id}/abort ───────────────────────────────────────

pub async fn stream_abort_handler(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(stream_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    let (_conv_id, _) = resolve_job(&state, stream_id, auth.user_id).await?;
    state.abort_job(stream_id).await;
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

    let (conversation_id, _) = resolve_job(&state, stream_id, auth.user_id).await?;

    // ── 1. Seal the in-flight streaming message ───────────────────────────
    // The worker writes tokens live into messages.streaming=true; sealing it
    // here (before interrupt_job) ensures the partial is in history before
    // the new worker's db.restore() runs.  Idempotent if worker beats us.
    let _ = sqlx::query(
        "UPDATE messages SET streaming = false WHERE job_id = $1 AND streaming = true",
    )
    .bind(stream_id)
    .execute(&state.pool)
    .await;

    // ── 2. Mark old job as 'interrupted' ─────────────────────────────────
    // Worker detects this status and exits without double-sealing.
    state.interrupt_job(stream_id).await;

    // ── 3. Persist the user's interrupt message synchronously ─────────────
    {
        use agentix::{Message, UserContent};
        state
            .persist_message_async(
                conversation_id,
                Message::User(vec![UserContent::Text { text: content }]),
            )
            .await;
    }

    // ── 4. Start new generation job immediately ───────────────────────────
    let new_job_id = state
        .start_generation(conversation_id, auth.user_id)
        .await
        .map_err(|e| AppError::internal(&e.to_string()))?;

    Ok((StatusCode::OK, Json(json!({ "stream_id": new_job_id }))))
}

// ── POST /api/stream/{stream_id}/answer ───────────────────────────────────────

#[derive(Deserialize)]
pub struct AnswerRequest {
    pub content: String,
}

/// Submit a user's answer to an `ask` tool call.
/// The old job is already done; we just persist the answer and start a new job.
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

    let (conversation_id, _) = resolve_job(&state, stream_id, auth.user_id).await?;

    // Persist the user's answer as a new message.
    {
        use agentix::{Message, UserContent};
        state
            .persist_message_async(
                conversation_id,
                Message::User(vec![UserContent::Text { text: content }]),
            )
            .await;
    }

    // Start a new generation job.
    let new_job_id = state
        .start_generation(conversation_id, auth.user_id)
        .await
        .map_err(|e| AppError::internal(&e.to_string()))?;

    Ok((StatusCode::OK, Json(json!({ "stream_id": new_job_id }))))
}

