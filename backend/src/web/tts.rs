use axum::{Json, extract::{Path, State}};
use base64::Engine as _;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::web::{AppState, auth::AuthUser};

#[derive(Debug, Serialize)]
pub struct MessageTtsResponse {
    pub audio_path: String,
    pub cached: bool,
}

#[derive(Debug, Deserialize)]
struct MimoChatResponse {
    choices: Vec<MimoChoice>,
}

#[derive(Debug, Deserialize)]
struct MimoChoice {
    message: MimoMessage,
}

#[derive(Debug, Deserialize)]
struct MimoMessage {
    audio: MimoAudio,
}

#[derive(Debug, Deserialize)]
struct MimoAudio {
    data: String,
}

pub async fn synthesize_message_tts(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((conversation_id, message_id)): Path<(Uuid, i64)>,
) -> AppResult<Json<MessageTtsResponse>> {
    let row = sqlx::query!(
        r#"
        SELECT m.content, m.tts_audio_path
        FROM messages m
        JOIN conversations c ON c.id = m.conversation_id
        WHERE m.id = $1
          AND m.conversation_id = $2
          AND c.user_id = $3
          AND m.role = 'assistant'
          AND m.streaming = false
        "#,
        message_id,
        conversation_id,
        auth.user_id
    )
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::not_found("可合成的 assistant 消息不存在"))?;

    if let Some(existing) = row.tts_audio_path {
        let file_path = std::path::PathBuf::from(&state.artifacts_path).join(&existing);
        if tokio::fs::metadata(&file_path).await.is_ok() {
            return Ok(Json(MessageTtsResponse { audio_path: format!("/artifacts/{existing}"), cached: true }));
        }
    }

    let text = row.content.unwrap_or_default();
    if text.trim().is_empty() {
        return Err(AppError::bad_request("assistant 消息内容为空，无法转语音"));
    }

    let api_key: Option<String> = sqlx::query_scalar(
        "SELECT mimo_api_key FROM app_config WHERE id = true",
    )
    .fetch_optional(&state.pool)
    .await?
    .flatten();
    let api_key = api_key
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| AppError::bad_request("未配置 MiMo API Key"))?;

    let body = serde_json::json!({
        "model": "mimo-v2.5-tts",
        "messages": [
            {
                "role": "user",
                "content": "用自然的河南话口音朗读，语气亲切、有生活感。不要解释，不要额外添加内容。"
            },
            {
                "role": "assistant",
                "content": format!("(河南话){}", text)
            }
        ],
        "audio": {
            "format": "wav",
            "voice": "mimo_default"
        }
    });

    let client = reqwest::Client::new();
    let resp = client
        .post("https://api.xiaomimimo.com/v1/chat/completions")
        .header("api-key", api_key)
        .json(&body)
        .send()
        .await
        .map_err(|e| AppError::internal(&format!("MiMo TTS 请求失败: {e}")))?;

    if resp.status() != StatusCode::OK {
        let status = resp.status();
        let err_text = resp.text().await.unwrap_or_default();
        return Err(AppError::internal(&format!("MiMo TTS 返回错误 {status}: {err_text}")));
    }

    let parsed: MimoChatResponse = resp
        .json()
        .await
        .map_err(|e| AppError::internal(&format!("MiMo TTS 响应解析失败: {e}")))?;
    let audio_b64 = parsed
        .choices
        .into_iter()
        .next()
        .ok_or_else(|| AppError::internal("MiMo TTS 响应缺少 choices"))?
        .message
        .audio
        .data;
    let audio_bytes = base64::engine::general_purpose::STANDARD
        .decode(audio_b64.as_bytes())
        .map_err(|e| AppError::internal(&format!("MiMo TTS 音频解码失败: {e}")))?;

    let rel_path = format!("tts/{message_id}.wav");
    let abs_path = std::path::PathBuf::from(&state.artifacts_path).join(&rel_path);
    if let Some(parent) = abs_path.parent() {
        tokio::fs::create_dir_all(parent).await
            .map_err(|e| AppError::internal(&format!("创建 TTS 目录失败: {e}")))?;
    }
    tokio::fs::write(&abs_path, audio_bytes).await
        .map_err(|e| AppError::internal(&format!("保存 TTS 音频失败: {e}")))?;

    sqlx::query!(
        r#"
        UPDATE messages
        SET tts_audio_path = $2,
            tts_voice = 'mimo_default',
            tts_style = '河南话',
            tts_generated_at = NOW()
        WHERE id = $1
        "#,
        message_id,
        rel_path
    )
    .execute(&state.pool)
    .await?;

    Ok(Json(MessageTtsResponse { audio_path: format!("/artifacts/{rel_path}"), cached: false }))
}
