use anyhow::Result;
use serde_json::json;

use crate::llm::ToolSpec;
use crate::sandbox;

pub fn tool_specs() -> Vec<ToolSpec> {
    vec![
        ToolSpec {
            name: "bash".into(),
            description: "Run a bash command in the conversation's workspace directory. \
                          Use this for shell, git, package managers, building, running scripts. \
                          Output is truncated at 64 KiB; commands time out after 120 seconds."
                .into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "Command line to execute via bash -lc" }
                },
                "required": ["command"],
            }),
        },
        ToolSpec {
            name: "read_file".into(),
            description: "Read a UTF-8 text file. Relative paths resolve to the workspace; \
                          absolute paths are read as-is."
                .into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" }
                },
                "required": ["path"],
            }),
        },
        ToolSpec {
            name: "write_file".into(),
            description: "Write a UTF-8 text file, creating parent directories as needed. \
                          Overwrites existing files. Relative paths resolve to the workspace."
                .into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "content": { "type": "string" }
                },
                "required": ["path", "content"],
            }),
        },
        ToolSpec {
            name: "list_dir".into(),
            description: "List entries in a directory (non-recursive).".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Directory path; defaults to workspace root" }
                },
            }),
        },
    ]
}

pub struct ToolOutcome {
    pub content: String,
    pub is_error: bool,
}

pub async fn dispatch(
    conversation_id: &str,
    name: &str,
    input: &serde_json::Value,
) -> ToolOutcome {
    let result = match name {
        "bash" => run_bash(conversation_id, input).await,
        "read_file" => run_read_file(conversation_id, input).await,
        "write_file" => run_write_file(conversation_id, input).await,
        "list_dir" => run_list_dir(conversation_id, input).await,
        other => Err(anyhow::anyhow!("unknown tool: {other}")),
    };
    match result {
        Ok(s) => ToolOutcome { content: s, is_error: false },
        Err(e) => ToolOutcome { content: format!("error: {e}"), is_error: true },
    }
}

async fn run_bash(conv: &str, input: &serde_json::Value) -> Result<String> {
    let cmd = input
        .get("command")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing 'command'"))?;
    let r = sandbox::run_bash(conv, cmd).await?;
    let mut out = String::new();
    if let Some(code) = r.exit_code {
        out.push_str(&format!("exit: {code}\n"));
    } else if r.timed_out {
        out.push_str("exit: timed out\n");
    } else {
        out.push_str("exit: signal\n");
    }
    if !r.stdout.is_empty() {
        out.push_str("--- stdout ---\n");
        out.push_str(&r.stdout);
        if !r.stdout.ends_with('\n') {
            out.push('\n');
        }
    }
    if !r.stderr.is_empty() {
        out.push_str("--- stderr ---\n");
        out.push_str(&r.stderr);
        if !r.stderr.ends_with('\n') {
            out.push('\n');
        }
    }
    Ok(out)
}

async fn run_read_file(conv: &str, input: &serde_json::Value) -> Result<String> {
    let path = input
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing 'path'"))?;
    let resolved = sandbox::resolve_in_workspace(conv, path)?;
    let bytes = tokio::fs::read(&resolved).await?;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    Ok(text)
}

async fn run_write_file(conv: &str, input: &serde_json::Value) -> Result<String> {
    let path = input
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing 'path'"))?;
    let content = input
        .get("content")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing 'content'"))?;
    let resolved = sandbox::resolve_in_workspace(conv, path)?;
    if let Some(parent) = resolved.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(&resolved, content).await?;
    Ok(format!("wrote {} bytes to {}", content.len(), resolved.display()))
}

async fn run_list_dir(conv: &str, input: &serde_json::Value) -> Result<String> {
    let path = input.get("path").and_then(|v| v.as_str()).unwrap_or(".");
    let resolved = sandbox::resolve_in_workspace(conv, path)?;
    let mut entries = tokio::fs::read_dir(&resolved).await?;
    let mut names = Vec::new();
    while let Some(entry) = entries.next_entry().await? {
        let kind = if entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false) {
            "/"
        } else {
            ""
        };
        names.push(format!("{}{kind}", entry.file_name().to_string_lossy()));
    }
    names.sort();
    Ok(names.join("\n"))
}
