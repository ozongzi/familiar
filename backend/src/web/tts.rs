use axum::{
    Json,
    extract::{Path, State},
};
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::Row;
use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::web::AppState;
use crate::web::auth::AuthUser;

#[derive(Debug, Deserialize)]
pub struct MessageTtsRequest {
    /// Frontend currently sends "(河南话)". Keeping this request-scoped lets
    /// future clients request another dialect without overwriting global config.
    pub style: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct MessageTtsResponse {
    pub url: String,
    pub path: String,
    pub cached: bool,
    pub model: String,
    pub voice: String,
    pub style: String,
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
    audio: Option<MimoAudio>,
}

#[derive(Debug, Deserialize)]
struct MimoAudio {
    data: String,
}

pub async fn synthesize_message_tts(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((conversation_id, message_id)): Path<(Uuid, i64)>,
    Json(req): Json<MessageTtsRequest>,
) -> AppResult<Json<MessageTtsResponse>> {
    let row = sqlx::query(
        r#"
        SELECT m.content
        FROM messages m
        JOIN conversations c ON c.id = m.conversation_id
        WHERE m.id = $1
          AND m.conversation_id = $2
          AND c.user_id = $3
          AND m.role = 'assistant'
          AND m.streaming = false
        "#,
    )
    .bind(message_id)
    .bind(conversation_id)
    .bind(auth.user_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::not_found("消息不存在或不可合成"))?;

    let content: String = row
        .try_get::<Option<String>, _>("content")?
        .unwrap_or_default();
    let content = content.trim();
    if content.is_empty() {
        return Err(AppError::bad_request("assistant 消息内容为空"));
    }

    let cfg = state.get_global_config().await?;
    let api_key = cfg
        .mimo_tts_api_key
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| AppError::bad_request("MiMo TTS API Key 未配置"))?
        .trim()
        .to_string();
    let api_base = cfg
        .mimo_tts_api_base
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("https://api.xiaomimimo.com/v1")
        .trim_end_matches('/')
        .to_string();
    let model = cfg
        .mimo_tts_model
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("mimo-v2.5-tts")
        .trim()
        .to_string();
    let voice = cfg
        .mimo_tts_voice
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("mimo_default")
        .trim()
        .to_string();
    let style = req
        .style
        .as_deref()
        .or(cfg.mimo_tts_style.as_deref())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("(河南话)")
        .trim()
        .to_string();
    let format = "wav".to_string();
    let content_hash = format!(
        "{:x}",
        md5::compute(
            format!("{api_base}\n{model}\n{voice}\n{style}\n{format}\n{content}").as_bytes()
        )
    );

    if let Some(path) = sqlx::query_scalar::<_, String>(
        r#"
        SELECT file_path
        FROM message_tts_cache
        WHERE message_id = $1
          AND model = $2
          AND voice = $3
          AND style = $4
          AND format = $5
          AND content_hash = $6
        LIMIT 1
        "#,
    )
    .bind(message_id)
    .bind(&model)
    .bind(&voice)
    .bind(&style)
    .bind(&format)
    .bind(&content_hash)
    .fetch_optional(&state.pool)
    .await?
    {
        let url = file_url(&path, conversation_id);
        return Ok(Json(MessageTtsResponse {
            url,
            path,
            cached: true,
            model,
            voice,
            style,
        }));
    }

    let assistant_content =
        if style.starts_with('(') || style.starts_with('（') || style.starts_with('[') {
            format!("{style}{content}")
        } else {
            format!("({style}){content}")
        };

    let body = json!({
        "model": &model,
        "messages": [
            {
                "role": "assistant",
                "content": &assistant_content
            }
        ],
        "audio": {
            "format": &format,
            "voice": &voice
        }
    });

    let resp = reqwest::Client::new()
        .post(format!("{api_base}/chat/completions"))
        .header("api-key", api_key)
        .json(&body)
        .send()
        .await
        .map_err(|e| AppError::internal(&format!("MiMo TTS 请求失败: {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(AppError::internal(&format!(
            "MiMo TTS 错误 {status}: {text}"
        )));
    }

    let parsed: MimoChatResponse = resp
        .json()
        .await
        .map_err(|e| AppError::internal(&format!("MiMo TTS 响应解析失败: {e}")))?;
    let audio_data = parsed
        .choices
        .first()
        .and_then(|c| c.message.audio.as_ref())
        .map(|a| a.data.as_str())
        .ok_or_else(|| AppError::internal("MiMo TTS 响应缺少 audio.data"))?;
    let audio_bytes = base64::engine::general_purpose::STANDARD
        .decode(audio_data)
        .map_err(|e| AppError::internal(&format!("MiMo TTS 音频解码失败: {e}")))?;

    let public_dir = state
        .sandbox
        .get_conversation_dir(auth.user_id, conversation_id)
        .join("public");
    tokio::fs::create_dir_all(&public_dir)
        .await
        .map_err(|e| AppError::internal(&format!("无法创建音频目录: {e}")))?;

    let filename = format!("tts-{content_hash}.wav");
    let path = format!("/workspace/public/{filename}");
    let host_path = public_dir.join(&filename);
    tokio::fs::write(&host_path, audio_bytes)
        .await
        .map_err(|e| AppError::internal(&format!("音频写入失败: {e}")))?;

    sqlx::query(
        r#"
        INSERT INTO message_tts_cache (message_id, model, voice, style, format, content_hash, file_path)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        ON CONFLICT (message_id, model, voice, style, format, content_hash)
        DO UPDATE SET file_path = EXCLUDED.file_path
        "#,
    )
    .bind(message_id)
    .bind(&model)
    .bind(&voice)
    .bind(&style)
    .bind(&format)
    .bind(&content_hash)
    .bind(&path)
    .execute(&state.pool)
    .await?;

    let url = file_url(&path, conversation_id);
    Ok(Json(MessageTtsResponse {
        url,
        path,
        cached: false,
        model,
        voice,
        style,
    }))
}

fn file_url(path: &str, conversation_id: Uuid) -> String {
    format!(
        "/api/files?path={}&conversation_id={}",
        url_component(path),
        conversation_id
    )
}

fn url_component(raw: &str) -> String {
    raw.replace('/', "%2F")
}
