use axum::{
    Json,
    extract::{Multipart, Query, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use axum_extra::{
    TypedHeader,
    headers::{Authorization, authorization::Bearer},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use tokio::fs::File;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;

use crate::errors::AppError;
use crate::web::AppState;

#[derive(Deserialize)]
pub struct FileQuery {
    path: String,
    /// Optional bearer token as query param (used when opening download links directly).
    token: Option<String>,
}

pub async fn download_file(
    State(state): State<AppState>,
    // Try the Authorization header first; token query param is the fallback.
    bearer: Option<TypedHeader<Authorization<Bearer>>>,
    Query(q): Query<FileQuery>,
) -> Result<Response, AppError> {
    // Resolve token: header takes priority, then query param.
    let token = bearer
        .as_ref()
        .map(|TypedHeader(Authorization(b))| b.token().to_string())
        .or_else(|| q.token.clone())
        .ok_or_else(AppError::unauthorized)?;

    // Validate the token and fetch user_id.
    let user_id: Uuid = sqlx::query_scalar("SELECT user_id FROM sessions WHERE token = $1")
        .bind(&token)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| {
            tracing::error!("file download auth query: {e}");
            AppError::internal("数据库错误")
        })?
        .ok_or_else(AppError::unauthorized)?;

    // Resolve to an absolute path.
    // If the path starts with /workspace, map it back to the host path.
    let q_path = std::path::PathBuf::from(&q.path);
    let path = if q_path.starts_with("/workspace") {
        let relative = q_path.strip_prefix("/workspace").unwrap();
        state.sandbox.get_user_dir(user_id).join(relative)
    } else {
        q_path
    };

    // Enforce ownership: path must be within the sandbox workspace.
    let user_dir = state.sandbox.get_user_dir(user_id);
    if !user_dir.exists() {
        tokio::fs::create_dir_all(&user_dir)
            .await
            .map_err(|e| AppError::internal(&format!("无法创建用户目录: {}", e)))?;
    }
    let canonical_user_dir = tokio::fs::canonicalize(&user_dir)
        .await
        .map_err(|_| AppError::not_found("用户目录无效"))?;
    // Canonicalize the requested path only if it exists; otherwise reject.
    let canonical_path = tokio::fs::canonicalize(&path)
        .await
        .map_err(|_| AppError::not_found("文件不存在"))?;
    if !canonical_path.starts_with(&canonical_user_dir) {
        return Err(AppError::not_found("文件不存在"));
    }

    let metadata = tokio::fs::metadata(&path).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            AppError::not_found("文件不存在")
        } else {
            AppError::internal(&e.to_string())
        }
    })?;

    if !metadata.is_file() {
        return Err(AppError::bad_request("路径不是一个文件"));
    }

    let mut file = tokio::fs::File::open(&path)
        .await
        .map_err(|e| AppError::internal(&e.to_string()))?;

    // Read the whole file into memory.
    // For the typical use-case (code files, logs) this is fine.
    // Large binary files will be truncated at 50 MB as a safeguard.
    const MAX_BYTES: u64 = 50 * 1024 * 1024;
    let size = metadata.len().min(MAX_BYTES);
    let mut buf = Vec::with_capacity(size as usize);
    file.read_to_end(&mut buf)
        .await
        .map_err(|e| AppError::internal(&e.to_string()))?;
    if buf.len() as u64 > MAX_BYTES {
        buf.truncate(MAX_BYTES as usize);
    }

    // Derive filename for Content-Disposition.
    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file")
        .to_string();

    // Guess MIME type from extension, fall back to octet-stream.
    let mime = mime_from_filename(&filename);

    // Build the response.
    let content_disposition = format!("attachment; filename=\"{}\"", filename.replace('"', "\\\""));

    let response = (
        [
            (header::CONTENT_TYPE, mime),
            (header::CONTENT_DISPOSITION, content_disposition.as_str()),
        ],
        buf,
    )
        .into_response();

    Ok(response)
}

// ─── Preview endpoint ─────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct PreviewResponse {
    filename: String,
    path: String,
    lang: String,
    line_count: usize,
    content: String,
    truncated: bool,
}

pub async fn preview_file(
    State(state): State<AppState>,
    bearer: Option<TypedHeader<Authorization<Bearer>>>,
    Query(q): Query<FileQuery>,
) -> Result<Json<PreviewResponse>, AppError> {
    let token = bearer
        .as_ref()
        .map(|TypedHeader(Authorization(b))| b.token().to_string())
        .or_else(|| q.token.clone())
        .ok_or_else(AppError::unauthorized)?;

    let user_id: Uuid = sqlx::query_scalar("SELECT user_id FROM sessions WHERE token = $1")
        .bind(&token)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| {
            tracing::error!("file preview auth query: {e}");
            AppError::internal("数据库错误")
        })?
        .ok_or_else(AppError::unauthorized)?;

    let q_path = std::path::PathBuf::from(&q.path);
    let path = if q_path.starts_with("/workspace") {
        let relative = q_path.strip_prefix("/workspace").unwrap();
        state.sandbox.get_user_dir(user_id).join(relative)
    } else {
        q_path
    };

    // Enforce ownership.
    let user_dir = state.sandbox.get_user_dir(user_id);
    let canonical_user_dir = tokio::fs::canonicalize(&user_dir)
        .await
        .map_err(|_| AppError::not_found("用户目录不存在"))?;
    let canonical_path = tokio::fs::canonicalize(&path)
        .await
        .map_err(|_| AppError::not_found("文件不存在"))?;
    if !canonical_path.starts_with(&canonical_user_dir) {
        return Err(AppError::not_found("文件不存在"));
    }

    let metadata = tokio::fs::metadata(&path).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            AppError::not_found("文件不存在")
        } else {
            AppError::internal(&e.to_string())
        }
    })?;

    if !metadata.is_file() {
        return Err(AppError::bad_request("路径不是一个文件"));
    }

    const MAX_PREVIEW: usize = 100 * 1024; // 100 KB

    let raw = tokio::fs::read(&path)
        .await
        .map_err(|e| AppError::internal(&e.to_string()))?;

    // Reject binary files: check for null bytes in first 8KB.
    let probe = &raw[..raw.len().min(8192)];
    if probe.contains(&0u8) {
        return Err(AppError::bad_request("二进制文件无法预览"));
    }

    let truncated = raw.len() > MAX_PREVIEW;
    let slice = &raw[..raw.len().min(MAX_PREVIEW)];
    let content = String::from_utf8_lossy(slice).into_owned();

    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file")
        .to_string();

    let lang = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| ext_to_lang(e).to_string())
        .unwrap_or_default();

    let line_count = content.lines().count();

    Ok(Json(PreviewResponse {
        filename,
        path: q.path.clone(),
        lang,
        line_count,
        content,
        truncated,
    }))
}

fn ext_to_lang(ext: &str) -> &'static str {
    match ext.to_ascii_lowercase().as_str() {
        "rs" => "rust",
        "js" | "mjs" | "cjs" => "javascript",
        "ts" | "mts" | "cts" => "typescript",
        "tsx" => "tsx",
        "jsx" => "jsx",
        "py" => "python",
        "sh" | "bash" | "zsh" => "bash",
        "fish" => "fish",
        "toml" => "toml",
        "yaml" | "yml" => "yaml",
        "json" => "json",
        "md" | "markdown" => "markdown",
        "html" | "htm" => "html",
        "css" => "css",
        "scss" | "sass" => "scss",
        "sql" => "sql",
        "c" => "c",
        "cpp" | "cc" | "cxx" => "cpp",
        "h" | "hpp" => "cpp",
        "go" => "go",
        "java" => "java",
        "kt" | "kts" => "kotlin",
        "swift" => "swift",
        "rb" => "ruby",
        "php" => "php",
        "lua" => "lua",
        "r" => "r",
        "dockerfile" => "dockerfile",
        "makefile" | "mk" => "makefile",
        "xml" | "svg" => "xml",
        "ini" | "cfg" | "conf" => "ini",
        "env" => "bash",
        _ => "",
    }
}

fn mime_from_filename(name: &str) -> &'static str {
    let ext = std::path::Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    match ext.to_ascii_lowercase().as_str() {
        // Text
        "txt" | "log" | "conf" | "ini" | "cfg" | "env" => "text/plain; charset=utf-8",
        "md" | "markdown" => "text/markdown; charset=utf-8",
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "csv" => "text/csv; charset=utf-8",
        // Code (served as plain text so browsers display rather than execute)
        "rs" | "py" | "js" | "ts" | "tsx" | "jsx" | "go" | "c" | "cpp" | "h" | "hpp" | "java"
        | "kt" | "swift" | "rb" | "php" | "lua" | "sh" | "bash" | "zsh" | "fish" | "sql"
        | "toml" | "yaml" | "yml" | "xml" | "svg" | "makefile" | "mk" => {
            "text/plain; charset=utf-8"
        }
        // Data
        "json" => "application/json",
        "pdf" => "application/pdf",
        // Images
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        // Archives
        "zip" => "application/zip",
        "tar" => "application/x-tar",
        "gz" => "application/gzip",
        // Fallback
        _ => "application/octet-stream",
    }
}

// ─── File upload handler ───────────────────────────────────────────────────

#[derive(Serialize)]
pub struct UploadResponse {
    filename: String,
    path: String,
    size: usize,
}

/// POST /api/files
/// Expects a multipart/form-data body with a "file" field.
/// Returns JSON with saved filename, path, and size.
pub async fn upload_file(
    State(state): State<AppState>,
    bearer: Option<TypedHeader<Authorization<Bearer>>>,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<UploadResponse>), AppError> {
    // Resolve token: header only (keep consistent with other endpoints)
    let token = bearer
        .as_ref()
        .map(|TypedHeader(Authorization(b))| b.token().to_string())
        .ok_or_else(AppError::unauthorized)?;

    // Validate the token and fetch user_id.
    let user_id: Uuid = sqlx::query_scalar("SELECT user_id FROM sessions WHERE token = $1")
        .bind(&token)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| {
            tracing::error!("file upload auth query: {e}");
            AppError::internal("数据库错误")
        })?
        .ok_or_else(AppError::unauthorized)?;

    // Storage directory scoped to the user (the sandbox workspace).
    let upload_dir = state.sandbox.get_user_dir(user_id);
    tokio::fs::create_dir_all(&upload_dir)
        .await
        .map_err(|e| AppError::internal(&e.to_string()))?;

    // Parse multipart fields and look for field named "file" and optional conversation_id.
    // We first collect fields so the conversation_id can appear before or after the file field.
    let mut file_name_opt: Option<String> = None;
    let mut file_data_opt: Option<bytes::Bytes> = None;
    let mut conv_id_opt: Option<Uuid> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::internal(&e.to_string()))?
    {
        if let Some(name) = field.name() {
            match name {
                "file" => {
                    file_name_opt = Some(
                        field
                            .file_name()
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| "upload.bin".to_string()),
                    );
                    let data = field
                        .bytes()
                        .await
                        .map_err(|e| AppError::internal(&e.to_string()))?;
                    file_data_opt = Some(data);
                }
                "conversation_id" => {
                    // treat as plain text field
                    if let Ok(text) = field.text().await
                        && let Ok(parsed) = Uuid::parse_str(text.trim())
                    {
                        conv_id_opt = Some(parsed);
                    }
                }
                _ => {
                    // ignore other fields
                }
            }
        }
    }

    let (file_name, data) = match (file_name_opt, file_data_opt) {
        (Some(n), Some(d)) => (n, d),
        _ => return Err(AppError::bad_request("未包含 file 字段")),
    };

    // Compose a unique safe filename (timestamp + sanitized original)
    let uniq = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let safe = file_name
        .replace("/", "_")
        .replace("\\", "_")
        .replace('"', "_");
    let unique_name = format!("{}-{}", uniq, safe);

    let dest_path = upload_dir.join(&unique_name);
    // In the sandbox, the user_dir is mounted at /workspace
    let sandbox_path = format!("/workspace/{}", unique_name);

    let mut f = File::create(&dest_path)
        .await
        .map_err(|e| AppError::internal(&e.to_string()))?;
    f.write_all(&data)
        .await
        .map_err(|e| AppError::internal(&e.to_string()))?;

    let resp = UploadResponse {
        filename: unique_name.clone(),
        path: sandbox_path.clone(),
        size: data.len(),
    };

    // If a conversation_id was provided, validate ownership and persist a User message
    // describing the uploaded file. Do NOT trigger model generation — only persist the message.
    if let Some(conv_id) = conv_id_opt {
        // Verify that the session token owner owns the conversation.
        let owned: bool = sqlx::query_scalar::<_, Option<bool>>(
            "SELECT EXISTS(SELECT 1 FROM conversations WHERE id = $1 AND user_id = (SELECT user_id FROM sessions WHERE token = $2))",
        )
        .bind(conv_id)
        .bind(&token)
        .fetch_one(&state.pool)
        .await
        .map_err(|e| {
            tracing::error!("file upload conv auth query: {e}");
            AppError::internal("数据库错误")
        })?
        .unwrap_or(false);

        if owned {
            // Persist a User-role message so DeepSeek's API is not violated
            // (Tool messages must follow assistant tool_calls; a spontaneous
            // Tool message would cause a 400 Bad Request).
            let content_str = json!({
                "__type": "file_upload",
                "filename": unique_name,
                "path": sandbox_path,
                "size": data.len(),
            })
            .to_string();

            use agentix::raw::request::message::{Message as AgentMessage, Role};
            let msg = AgentMessage::new(Role::User, &content_str);
            // Persist to DB.
            state.persist_message(conv_id, &msg);

            // Also push into the in-memory agent if it is currently idle
            // (not mid-generation). This ensures the next generation turn
            // sees the uploaded file without having to rebuild the agent.
            {
                let mut map = state.chats.lock().unwrap();
                if let Some(entry) = map.get_mut(&conv_id)
                    && let Some(ref mut agent) = entry.agent
                {
                    agent.push_user_message(&content_str);
                }
            }
        } else {
            tracing::warn!(
                "upload attempted for conversation not owned by token: {}",
                conv_id
            );
            // still return 201 for the upload itself, but do not persist to conversation
        }
    }

    Ok((StatusCode::CREATED, Json(resp)))
}

// ─── Avatar Upload ────────────────────────────────────────────────────────────

/// POST /api/users/me/avatar
/// Upload avatar image for the current user
pub async fn upload_avatar(
    State(state): State<AppState>,
    bearer: Option<TypedHeader<Authorization<Bearer>>>,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<serde_json::Value>), AppError> {
    let token = bearer
        .as_ref()
        .map(|TypedHeader(Authorization(b))| b.token().to_string())
        .ok_or_else(AppError::unauthorized)?;

    let user_id: Uuid = sqlx::query_scalar("SELECT user_id FROM sessions WHERE token = $1")
        .bind(&token)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| {
            tracing::error!("avatar upload auth query: {e}");
            AppError::internal("数据库错误")
        })?
        .ok_or_else(AppError::unauthorized)?;

    // Get the avatars directory
    let avatars_dir = std::path::PathBuf::from(&state.artifacts_path).join("avatars");
    tokio::fs::create_dir_all(&avatars_dir)
        .await
        .map_err(|e| AppError::internal(&format!("无法创建头像目录: {}", e)))?;

    // Parse multipart to get file
    let mut file_data: Option<bytes::Bytes> = None;
    let mut file_ext: Option<String> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::bad_request(&format!("multipart 解析错误: {}", e)))?
    {
        if let Some(name) = field.name()
            && (name == "avatar" || name == "file")
        {
            // Get original filename to extract extension
            if let Some(filename) = field.file_name() {
                let ext = std::path::Path::new(filename)
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.to_lowercase());

                // Validate file extension
                match ext.as_deref() {
                    Some("jpg") | Some("jpeg") | Some("png") | Some("webp") => {
                        file_ext = ext;
                    }
                    _ => {
                        return Err(AppError::bad_request("仅支持 JPG、PNG 或 WebP 格式"));
                    }
                }
            }

            let data = field
                .bytes()
                .await
                .map_err(|e| AppError::internal(&format!("读取文件数据错误: {}", e)))?;

            // Validate file size (max 2MB)
            const MAX_SIZE: usize = 2 * 1024 * 1024;
            if data.len() > MAX_SIZE {
                return Err(AppError::bad_request("文件大小不能超过 2MB"));
            }

            file_data = Some(data);
            break;
        }
    }

    let (data, ext) = match (file_data, file_ext) {
        (Some(d), Some(e)) => (d, e),
        _ => return Err(AppError::bad_request("未找到有效的头像文件")),
    };

    // Get old avatar path to delete it
    let old_avatar: Option<String> =
        sqlx::query_scalar("SELECT avatar_path FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_optional(&state.pool)
            .await?;

    // Generate new filename: <user_id>.<ext>
    let avatar_filename = format!("{}.{}", user_id, ext);
    let avatar_path = avatars_dir.join(&avatar_filename);
    let avatar_db_path = format!("avatars/{}", avatar_filename);

    // Save the file
    let mut file = File::create(&avatar_path)
        .await
        .map_err(|e| AppError::internal(&format!("保存头像失败: {}", e)))?;
    file.write_all(&data)
        .await
        .map_err(|e| AppError::internal(&format!("写入头像失败: {}", e)))?;

    // Update database
    sqlx::query("UPDATE users SET avatar_path = $1 WHERE id = $2")
        .bind(&avatar_db_path)
        .bind(user_id)
        .execute(&state.pool)
        .await?;

    // Delete old avatar file if it exists and is different
    if let Some(old_path) = old_avatar
        && old_path != avatar_db_path
    {
        let old_file = std::path::PathBuf::from(&state.artifacts_path).join(&old_path);
        let _ = tokio::fs::remove_file(old_file).await;
    }

    // Log audit
    let _ = crate::audit::log_audit(
        &state.pool,
        Some(user_id),
        Some(user_id),
        "upload_avatar",
        Some(json!({ "filename": avatar_filename })),
        None,
    )
    .await;

    Ok((
        StatusCode::OK,
        Json(json!({
            "avatar_path": avatar_db_path,
            "message": "头像上传成功"
        })),
    ))
}

/// GET /api/avatars/:user_id
/// Serve user avatar image
pub async fn get_avatar(
    State(state): State<AppState>,
    axum::extract::Path(user_id): axum::extract::Path<Uuid>,
) -> Result<Response, AppError> {
    // Get avatar path from database
    let avatar_path: Option<String> =
        sqlx::query_scalar("SELECT avatar_path FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_optional(&state.pool)
            .await?
            .ok_or_else(|| AppError::not_found("用户不存在"))?;

    let avatar_path = avatar_path.ok_or_else(|| AppError::not_found("用户未设置头像"))?;

    // Build full file path
    let file_path = std::path::PathBuf::from(&state.artifacts_path).join(&avatar_path);

    // Read file
    let data = tokio::fs::read(&file_path)
        .await
        .map_err(|_| AppError::not_found("头像文件不存在"))?;

    // Determine MIME type from extension
    let mime = mime_from_filename(&avatar_path);

    // Set cache headers for better performance
    let response = (
        [
            (header::CONTENT_TYPE, mime),
            (header::CACHE_CONTROL, "public, max-age=86400"), // 1 day cache
        ],
        data,
    )
        .into_response();

    Ok(response)
}
