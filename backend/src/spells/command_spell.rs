use ds_api::tool;
use serde_json::json;
use tokio::process::Command;
use tokio::time::Duration;

use crate::spells::execute_command;

pub struct CommandSpell;

#[tool]
impl Tool for CommandSpell {
    /// 跨平台执行终端命令
    /// command: 需要执行的终端命令
    /// cwd: 工作目录（可选），不传则使用服务器默认目录
    /// timeout_secs: 超时时间（秒，可选，默认为 20）
    async fn execute(
        &self,
        command: String,
        cwd: Option<String>,
        timeout_secs: Option<u64>,
    ) -> Value {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(&command);

        if let Some(dir) = cwd {
            // If provided cwd is relative, resolve it relative to the default_home and canonicalize.
            match std::fs::canonicalize(dir) {
                Ok(cwd) => {
                    cmd.current_dir(cwd);
                }
                Err(e) => return json!(format!("Error: {e}")),
            }
        }

        let timeout_time = Duration::from_secs(timeout_secs.unwrap_or(20));

        execute_command(cmd, timeout_time).await
    }
}
