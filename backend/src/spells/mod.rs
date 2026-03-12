mod file_spells;
mod history_spell;
mod search_spells;
mod shell_spells;
mod spawn_spell;
mod ui_spells;

use std::time::Duration;

pub use ds_api::tool_trait::ToolBundle;
pub use file_spells::FileSpells;
pub use history_spell::HistorySpell;
pub use search_spells::SearchSpells;
pub use shell_spells::ShellSpells;
pub use spawn_spell::SpawnSpell;
pub use ui_spells::UiSpells;

use serde_json::{Value, json};
use tokio::{process::Command, time::timeout};

pub const MAX_OUTPUT_CHARS: usize = 8_000;

/// 大文件自动降级到 outline 的行数阈值
pub(crate) const OUTLINE_THRESHOLD: usize = 300;

/// 超长输出保留头尾，中间用省略提示替换
pub(crate) fn truncate_output(s: &str, max_chars: usize) -> String {
    if s.len() <= max_chars {
        return s.to_string();
    }
    let half = max_chars / 2;
    let head = &s[..half];
    let tail_start = s.len() - half;
    let tail = &s[tail_start..];
    format!(
        "{}\n\n... [输出过长，中间 {} 字节已省略] ...\n\n{}",
        head,
        s.len() - max_chars,
        tail
    )
}

pub(crate) async fn run_cmd(mut cmd: Command, timeout_time: Duration) -> Value {
    cmd.kill_on_drop(true);
    match timeout(timeout_time, cmd.output()).await {
        Err(_) => json!({ "error": "命令超时" }),
        Ok(Err(e)) => json!({ "error": e.to_string() }),
        Ok(Ok(out)) => json!({
            "stdout": truncate_output(String::from_utf8_lossy(&out.stdout).trim(), MAX_OUTPUT_CHARS),
            "stderr": truncate_output(String::from_utf8_lossy(&out.stderr).trim(), MAX_OUTPUT_CHARS),
            "exit_code": out.status.code(),
        }),
    }
}

pub(crate) use search_spells::outline_value;
