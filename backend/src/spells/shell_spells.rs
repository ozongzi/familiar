use std::time::Duration;

use ds_api::tool;
use serde_json::json;
use tokio::process::Command;

pub struct ShellSpells;

#[tool]
impl Tool for ShellSpells {
    /// 执行 shell 命令（通过 sh -c，支持管道和重定向）。
    /// 适合 git、构建、测试、安装依赖等操作。
    /// 能用 read / edit / search / glob 解决的任务请优先用专用工具，bash 作通用后备。
    ///
    /// description: 本次操作意图（供 UI 渲染，可不填）
    /// command: 要执行的命令
    /// cwd: 工作目录（可选）
    /// timeout_secs: 超时秒数（可选，默认 30）
    async fn bash(
        &self,
        description: Option<String>,
        command: String,
        cwd: Option<String>,
        timeout_secs: Option<u64>,
    ) -> Value {
        let _ = description;
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(&command);
        if let Some(dir) = cwd {
            match tokio::fs::canonicalize(&dir).await {
                Ok(p) => {
                    cmd.current_dir(p);
                }
                Err(e) => return json!({ "error": format!("无效工作目录 '{dir}': {e}") }),
            }
        }
        super::run_cmd(cmd, Duration::from_secs(timeout_secs.unwrap_or(30))).await
    }
}
