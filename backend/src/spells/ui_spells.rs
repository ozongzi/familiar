use std::sync::Arc;
use std::time::Duration;

use ds_api::tool;
use serde_json::json;
use tokio::sync::Mutex;

pub struct UiSpells {
    /// oneshot slot：等待用户回答时写入，ws.rs 收到 answer 后触发
    pub ask_pending: Arc<Mutex<Option<tokio::sync::oneshot::Sender<String>>>>,
}

#[tool]
impl Tool for UiSpells {
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
