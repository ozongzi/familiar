use std::sync::Arc;
use std::time::Duration;

use ds_api::tool;
use serde_json::json;
use tokio::sync::Mutex;

pub struct UiSpells {
    /// oneshot slot：等待用户回答时写入，ws.rs 收到 answer 后触发
    pub ask_pending: Arc<Mutex<Option<tokio::sync::oneshot::Sender<String>>>>,
    /// Sandbox manager to resolve file paths and sizes
    pub sandbox: Arc<crate::sandbox::SandboxManager>,
    /// The authenticated user's id.
    pub user_id: uuid::Uuid,
}

#[tool]
impl Tool for UiSpells {
    /// 将文件展示给用户（在 UI 中渲染为一个类似 Claude 的文件卡片，支持预览和下载）。
    /// 适合展示生成的图表、导出的数据、或者需要用户重点关注的代码文件。
    ///
    /// description: 本次展示的简短说明（例如：“我为你生成了数据分析报告”）
    /// path: 文件的完整路径（通常以 /workspace/ 开头）
    async fn present_file(&self, description: Option<String>, path: String) -> Value {
        let _ = description;
        let q_path = std::path::PathBuf::from(&path);
        
        // Map sandbox path back to host path to get file size
        let host_path = if q_path.starts_with("/workspace") {
            let relative = q_path.strip_prefix("/workspace").unwrap();
            self.sandbox.get_user_dir(self.user_id).join(relative)
        } else {
            q_path
        };

        let filename = host_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file")
            .to_string();

        let size = tokio::fs::metadata(&host_path)
            .await
            .map(|m| m.len())
            .unwrap_or(0);

        json!({
            "display": "file",
            "filename": filename,
            "path": path, // keep the sandbox path for the frontend
            "size": size
        })
    }

    /// 向用户提问并等待回答后再继续。
    /// 适合需要确认、选择或补充信息的场景。
    /// 有 options 时前端渲染为快捷按钮。
    ///
    /// description: 本次操作意图（供 UI 渲染，可不填）
    /// question: 向用户展示的问题文本
    /// options: 供用户快速选择的选项（可选）
    async fn ask(
        &self,
        description: Option<String>,
        question: String,
        options: Option<Vec<String>>,
    ) -> Value {
        let _ = (description, options, question);
        let (tx, rx) = tokio::sync::oneshot::channel::<String>();
        *self.ask_pending.lock().await = Some(tx);
        match tokio::time::timeout(Duration::from_secs(300), rx).await {
            Ok(Ok(answer)) => json!({ "answer": answer }),
            Ok(Err(_)) => json!({ "error": "连接已关闭，用户未作答" }),
            Err(_) => json!({ "error": "等待超时（5 分钟）" }),
        }
    }

}
