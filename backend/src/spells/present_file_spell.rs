use ds_api::tool;
use serde_json::json;

pub struct PresentFileSpell;

#[tool]
impl Tool for PresentFileSpell {
    /// 将服务器上的文件提供给用户下载。
    /// 适合展示代码、日志、生成的图片等任何文件。
    /// path: 要下载的文件的绝对路径或相对路径
    async fn present_file(&self, path: String) -> Value {
        let metadata = match tokio::fs::metadata(&path).await {
            Ok(m) => m,
            Err(e) => return json!({ "error": e.to_string() }),
        };

        if !metadata.is_file() {
            return json!({ "error": format!("'{}' 不是一个文件", path) });
        }

        let filename = std::path::Path::new(&path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file")
            .to_string();

        // Return metadata only — the web layer exposes a /api/files endpoint
        // that accepts `path` as a query parameter, verifies the session token,
        // and streams the file with Content-Disposition: attachment.
        json!({
            "display": "file",
            "filename": filename,
            "path": path,
            "size": metadata.len(),
        })
    }
}
