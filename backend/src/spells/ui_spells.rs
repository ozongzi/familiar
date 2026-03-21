use std::sync::Arc;
use std::time::Duration;

use agentix::tool;
use serde_json::json;
use tokio::sync::Mutex;

pub struct UiSpells {
    pub ask_pending: Arc<Mutex<Option<tokio::sync::oneshot::Sender<String>>>>,
    pub sandbox: Arc<crate::sandbox::SandboxManager>,
    pub user_id: uuid::Uuid,
}

#[tool]
impl Tool for UiSpells {
    /// 将文件展示给用户（在 UI 中渲染为一个类似 Claude 的文件卡片，支持预览和下载）。
    /// 适合展示生成的图表、导出的数据、或者需要用户重点关注的代码文件。
    ///
    /// description: 本次展示的简短说明（例如："我为你生成了数据分析报告"）
    /// path: 文件的完整路径（通常以 /workspace/ 开头）
    async fn present_file(&self, description: Option<String>, path: String) -> serde_json::Value {
        let _ = description;
        let q_path = std::path::PathBuf::from(&path);
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
            "path": path,
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
    ) -> serde_json::Value {
        let _ = (description, options, question);
        let (tx, rx) = tokio::sync::oneshot::channel::<String>();
        *self.ask_pending.lock().await = Some(tx);
        match tokio::time::timeout(Duration::from_secs(300), rx).await {
            Ok(Ok(answer)) => json!({ "answer": answer }),
            Ok(Err(_)) => json!({ "error": "连接已关闭，用户未作答" }),
            Err(_) => json!({ "error": "等待超时（5 分钟）" }),
        }
    }

    /// 在对话中内嵌渲染一个交互式 widget（图表、可视化、计算器等）。
    /// 适合用来展示数据图表、复利计算器、流程图等视觉内容。
    ///
    /// widget_code: 完整的 HTML 代码片段（可使用 Chart.js、D3 等 CDN 库）。
    ///   不要包含 <!DOCTYPE>、<html>、<head>、<body> 标签，直接写内容。
    ///   可使用 CSS 变量：--text-primary、--bg-surface、--accent 等与 familiar 主题一致。
    async fn visualize(&self, widget_code: String) -> serde_json::Value {
        json!({ "status": "success" })
    }
}
