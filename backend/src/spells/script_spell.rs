use std::time::Duration;

use ds_api::tool;
use serde_json::json;
use tokio::process::Command;
use uuid::Uuid;

use crate::spells::execute_command;

pub struct ScriptSpell;

#[tool]
impl Tool for ScriptSpell {
    /// 运行 TypeScript 脚本（使用 Bun 作为运行时）。
    /// 可直接在脚本顶部用 import 引入 npm 包，Bun 会自动安装，无需额外声明依赖。
    /// 示例：import { format } from "date-fns";
    /// script: 脚本内容
    /// timeout: 超时时间，默认 20 秒
    async fn run_ts(&self, script: String, timeout: Option<u64>) -> Value {
        let id = Uuid::new_v4().simple();
        let tmp_path = format!("/tmp/familiar_{id}.ts");
        let timeout = timeout.unwrap_or(20);

        if let Err(e) = tokio::fs::write(&tmp_path, &script).await {
            return json!({ "error": format!("写入脚本失败: {}", e) });
        }

        let mut cmd = Command::new("bun");
        cmd.arg(&tmp_path);

        let result = execute_command(cmd, Duration::from_secs(timeout)).await;
        let _ = tokio::fs::remove_file(&tmp_path).await;
        result
    }

    /// 运行 Python 脚本（使用 uv run 作为运行时）。
    /// 可在脚本顶部用 PEP 723 inline metadata 声明依赖，uv 会自动安装：
    ///
    /// # /// script
    /// # requires-python = ">=3.11"
    /// # dependencies = ["requests", "rich>=13"]
    /// # ///
    ///
    /// script: 脚本内容
    /// timeout: 超时时间，默认 20 秒
    async fn run_py(&self, script: String, timeout: Option<u64>) -> Value {
        let id = Uuid::new_v4().simple();
        let tmp_path = format!("/tmp/familiar_{id}.py");
        let timeout = timeout.unwrap_or(20);

        if let Err(e) = tokio::fs::write(&tmp_path, &script).await {
            return json!({ "error": format!("写入脚本失败: {}", e) });
        }

        let mut cmd = Command::new("uv");
        cmd.args(["run", &tmp_path]);

        let result = execute_command(cmd, Duration::from_secs(timeout)).await;
        let _ = tokio::fs::remove_file(&tmp_path).await;
        result
    }
}
