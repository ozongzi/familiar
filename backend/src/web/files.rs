use axum::{
    Json,
    extract::{Query, State},
    http::header,
    response::{IntoResponse, Response},
};
use axum_extra::{
    TypedHeader,
    headers::{Authorization, authorization::Bearer},
};
use serde::{Deserialize, Serialize};

use tokio::io::AsyncReadExt;

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

    // Validate the token against the sessions table.
    sqlx::query("SELECT user_id FROM sessions WHERE token = $1")
        .bind(&token)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| {
            tracing::error!("file download auth query: {e}");
            AppError::internal("数据库错误")
        })?
        .ok_or_else(AppError::unauthorized)?;

    // Resolve to an absolute path (relative paths are from the process working dir).
    let path = std::path::PathBuf::from(&q.path);

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

    sqlx::query("SELECT user_id FROM sessions WHERE token = $1")
        .bind(&token)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| {
            tracing::error!("file preview auth query: {e}");
            AppError::internal("数据库错误")
        })?
        .ok_or_else(AppError::unauthorized)?;

    let path = std::path::PathBuf::from(&q.path);

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
