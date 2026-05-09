// "Sandbox" is a misnomer for the local client — there is no isolation.
// This module just keeps a per-conversation working directory and runs
// subprocesses inside it. The user's filesystem is the user's filesystem.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::process::Command;
use tokio::time::timeout;

use crate::storage::paths;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_OUTPUT: usize = 64 * 1024;

pub fn workspace(conversation_id: &str) -> Result<PathBuf> {
    let dir = paths::workspace_for(conversation_id);
    std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    Ok(dir)
}

pub struct ShellResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
}

pub async fn run_bash(conversation_id: &str, command: &str) -> Result<ShellResult> {
    let cwd = workspace(conversation_id)?;

    #[cfg(unix)]
    let mut cmd = {
        let mut c = Command::new("/bin/bash");
        c.arg("-lc").arg(command);
        c
    };
    #[cfg(windows)]
    let mut cmd = {
        let mut c = Command::new("cmd");
        c.arg("/C").arg(command);
        c
    };

    cmd.current_dir(&cwd)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);

    let child = cmd.spawn().context("spawn shell")?;
    let fut = child.wait_with_output();

    match timeout(DEFAULT_TIMEOUT, fut).await {
        Ok(Ok(out)) => Ok(ShellResult {
            stdout: truncate(String::from_utf8_lossy(&out.stdout).into_owned()),
            stderr: truncate(String::from_utf8_lossy(&out.stderr).into_owned()),
            exit_code: out.status.code(),
            timed_out: false,
        }),
        Ok(Err(e)) => Err(e.into()),
        Err(_) => Ok(ShellResult {
            stdout: String::new(),
            stderr: format!("(timed out after {}s)", DEFAULT_TIMEOUT.as_secs()),
            exit_code: None,
            timed_out: true,
        }),
    }
}

fn truncate(mut s: String) -> String {
    if s.len() > MAX_OUTPUT {
        s.truncate(MAX_OUTPUT);
        s.push_str("\n…(truncated)");
    }
    s
}

/// Resolve a user-provided path against the conversation's workspace.
/// Absolute paths are honored as-is (no jail); relative ones land in the workspace.
pub fn resolve_in_workspace(conversation_id: &str, path: &str) -> Result<PathBuf> {
    let p = PathBuf::from(path);
    if p.is_absolute() {
        Ok(p)
    } else {
        Ok(workspace(conversation_id)?.join(p))
    }
}
