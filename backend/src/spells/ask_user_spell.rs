use ds_api::tool;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct AskUserSpell {
    /// Shared slot: the spell writes a oneshot sender here while waiting,
    /// and ws.rs extracts and fires it when the user replies.
    pub pending: Arc<Mutex<Option<tokio::sync::oneshot::Sender<String>>>>,
}

#[tool]
impl Tool for AskUserSpell {
    /// 向用户提问并等待用户回答后再继续。适用于需要用户确认、选择选项或提供信息的情况。
    /// question: 向用户展示的问题文本
    /// options: 可供用户快速选择的选项列表（可选）
    async fn ask_user(&self, question: String, options: Option<Vec<String>>) -> Value {
        let (tx, rx) = tokio::sync::oneshot::channel::<String>();
        {
            let mut guard = self.pending.lock().await;
            *guard = Some(tx);
        }
        let _ = options; // only used by the frontend to render option buttons

        const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300); // 5 min
        match tokio::time::timeout(TIMEOUT, rx).await {
            Ok(Ok(answer)) => json!({ "answer": answer }),
            Ok(Err(_)) => json!({ "error": "用户未作答（连接已关闭）" }),
            Err(_) => json!({ "error": "等待超时（5 分钟）" }),
        }
    }
}
