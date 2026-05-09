// Conversation persistence: one .md file per conversation.
//
// Format (loss-less round-trip):
//
//   ---
//   id: 01J...
//   title: ...
//   created_at: 2026-05-09T10:00:00Z
//   updated_at: 2026-05-09T10:00:00Z
//   model: claude-sonnet-4-6
//   ---
//
//   # user
//   hello
//
//   # assistant
//   let me look at that file.
//
//   <!-- tool_use id=toolu_01 name=read_file -->
//   {"path": "foo.txt"}
//   <!-- /tool_use -->
//
//   <!-- tool_result id=toolu_01 -->
//   contents of foo.txt
//   <!-- /tool_result -->
//
//   done!
//
// Tool blocks use HTML comments so any markdown viewer renders the file
// cleanly. Headers (`# user`, `# assistant`) start each turn. Tool blocks
// belong to the most recent assistant turn.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::paths;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Block {
    Text { text: String },
    ToolUse { id: String, name: String, input: serde_json::Value },
    ToolResult { id: String, content: String, is_error: bool },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Turn {
    pub role: Role,
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Meta {
    pub id: String,
    pub title: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub model: String,
}

#[derive(Debug, Clone)]
pub struct Conversation {
    pub meta: Meta,
    pub turns: Vec<Turn>,
}

impl Conversation {
    pub fn new(model: &str) -> Self {
        let id = ulid::Ulid::new().to_string();
        let now = Utc::now();
        Self {
            meta: Meta {
                id,
                title: "新对话".to_string(),
                created_at: now,
                updated_at: now,
                model: model.to_string(),
            },
            turns: Vec::new(),
        }
    }

    pub fn path(&self) -> PathBuf {
        paths::conversations_dir().join(format!("{}.md", self.meta.id))
    }

    pub fn save(&self) -> Result<()> {
        let path = self.path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let body = serialize(self);
        std::fs::write(&path, body).with_context(|| format!("write {}", path.display()))?;
        Ok(())
    }

    pub fn load(id: &str) -> Result<Self> {
        let path = paths::conversations_dir().join(format!("{id}.md"));
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("read {}", path.display()))?;
        parse(&raw)
    }

    pub fn list_all() -> Vec<Meta> {
        let dir = paths::conversations_dir();
        let mut out: Vec<Meta> = Vec::new();
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return out;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e != "md").unwrap_or(true) {
                continue;
            }
            if let Ok(raw) = std::fs::read_to_string(&path)
                && let Ok(meta) = parse_frontmatter_only(&raw)
            {
                out.push(meta);
            }
        }
        out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        out
    }

    pub fn delete(id: &str) -> Result<()> {
        let path = paths::conversations_dir().join(format!("{id}.md"));
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        Ok(())
    }
}

// ── serialization ────────────────────────────────────────────────────────────

fn serialize(c: &Conversation) -> String {
    let mut out = String::new();
    out.push_str("---\n");
    out.push_str(&serde_yaml::to_string(&c.meta).unwrap_or_default());
    out.push_str("---\n\n");

    for turn in &c.turns {
        let header = match turn.role {
            Role::User => "# user",
            Role::Assistant => "# assistant",
        };
        out.push_str(header);
        out.push('\n');

        for block in &turn.blocks {
            match block {
                Block::Text { text } => {
                    out.push('\n');
                    out.push_str(text.trim_end());
                    out.push('\n');
                }
                Block::ToolUse { id, name, input } => {
                    out.push_str(&format!("\n<!-- tool_use id={id} name={name} -->\n"));
                    out.push_str(
                        &serde_json::to_string_pretty(input).unwrap_or_else(|_| "{}".into()),
                    );
                    out.push_str("\n<!-- /tool_use -->\n");
                }
                Block::ToolResult { id, content, is_error } => {
                    let err = if *is_error { " error=true" } else { "" };
                    out.push_str(&format!("\n<!-- tool_result id={id}{err} -->\n"));
                    out.push_str(content.trim_end());
                    out.push_str("\n<!-- /tool_result -->\n");
                }
            }
        }
        out.push('\n');
    }

    out
}

fn parse_frontmatter_only(raw: &str) -> Result<Meta> {
    let (meta_str, _) = split_frontmatter(raw)?;
    let meta: Meta = serde_yaml::from_str(meta_str).context("parse frontmatter")?;
    Ok(meta)
}

fn split_frontmatter(raw: &str) -> Result<(&str, &str)> {
    let raw = raw.strip_prefix("---\n").or_else(|| raw.strip_prefix("---\r\n"))
        .ok_or_else(|| anyhow::anyhow!("missing frontmatter opener"))?;
    let end = raw.find("\n---").ok_or_else(|| anyhow::anyhow!("missing frontmatter closer"))?;
    let meta = &raw[..end];
    let body = &raw[end + 4..];
    let body = body.strip_prefix('\n').unwrap_or(body);
    Ok((meta, body))
}

pub fn parse(raw: &str) -> Result<Conversation> {
    let (meta_str, body) = split_frontmatter(raw)?;
    let meta: Meta = serde_yaml::from_str(meta_str).context("parse frontmatter")?;

    let mut turns: Vec<Turn> = Vec::new();
    let mut current_role: Option<Role> = None;
    let mut current_blocks: Vec<Block> = Vec::new();
    let mut text_buf = String::new();

    let mut lines = body.lines().peekable();
    while let Some(line) = lines.next() {
        // Section header
        if let Some(role) = parse_role_header(line) {
            flush_text(&mut text_buf, &mut current_blocks);
            if let Some(prev) = current_role.take() {
                turns.push(Turn { role: prev, blocks: std::mem::take(&mut current_blocks) });
            }
            current_role = Some(role);
            continue;
        }

        // Tool use opener
        if let Some((id, name)) = parse_tool_use_open(line) {
            flush_text(&mut text_buf, &mut current_blocks);
            let mut buf = String::new();
            let mut closed = false;
            for inner in lines.by_ref() {
                if inner.trim() == "<!-- /tool_use -->" {
                    closed = true;
                    break;
                }
                buf.push_str(inner);
                buf.push('\n');
            }
            if !closed {
                bail!("unterminated tool_use block");
            }
            let input: serde_json::Value =
                serde_json::from_str(buf.trim()).unwrap_or(serde_json::json!({}));
            current_blocks.push(Block::ToolUse { id, name, input });
            continue;
        }

        // Tool result opener
        if let Some((id, is_error)) = parse_tool_result_open(line) {
            flush_text(&mut text_buf, &mut current_blocks);
            let mut buf = String::new();
            let mut closed = false;
            for inner in lines.by_ref() {
                if inner.trim() == "<!-- /tool_result -->" {
                    closed = true;
                    break;
                }
                buf.push_str(inner);
                buf.push('\n');
            }
            if !closed {
                bail!("unterminated tool_result block");
            }
            current_blocks.push(Block::ToolResult {
                id,
                content: buf.trim_end().to_string(),
                is_error,
            });
            continue;
        }

        text_buf.push_str(line);
        text_buf.push('\n');
    }

    flush_text(&mut text_buf, &mut current_blocks);
    if let Some(role) = current_role {
        turns.push(Turn { role, blocks: current_blocks });
    }

    Ok(Conversation { meta, turns })
}

fn flush_text(buf: &mut String, blocks: &mut Vec<Block>) {
    let trimmed = buf.trim();
    if !trimmed.is_empty() {
        blocks.push(Block::Text { text: trimmed.to_string() });
    }
    buf.clear();
}

fn parse_role_header(line: &str) -> Option<Role> {
    match line.trim() {
        "# user" => Some(Role::User),
        "# assistant" => Some(Role::Assistant),
        _ => None,
    }
}

fn parse_tool_use_open(line: &str) -> Option<(String, String)> {
    // <!-- tool_use id=XXX name=YYY -->
    let line = line.trim();
    let inner = line.strip_prefix("<!-- tool_use ")?.strip_suffix(" -->")?;
    let mut id = None;
    let mut name = None;
    for kv in inner.split_whitespace() {
        if let Some(v) = kv.strip_prefix("id=") {
            id = Some(v.to_string());
        } else if let Some(v) = kv.strip_prefix("name=") {
            name = Some(v.to_string());
        }
    }
    Some((id?, name?))
}

fn parse_tool_result_open(line: &str) -> Option<(String, bool)> {
    // <!-- tool_result id=XXX [error=true] -->
    let line = line.trim();
    let inner = line.strip_prefix("<!-- tool_result ")?.strip_suffix(" -->")?;
    let mut id = None;
    let mut err = false;
    for kv in inner.split_whitespace() {
        if let Some(v) = kv.strip_prefix("id=") {
            id = Some(v.to_string());
        } else if let Some(v) = kv.strip_prefix("error=") {
            err = v == "true";
        }
    }
    Some((id?, err))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let mut c = Conversation::new("claude-sonnet-4-6");
        c.meta.title = "test".into();
        c.turns.push(Turn {
            role: Role::User,
            blocks: vec![Block::Text { text: "hello".into() }],
        });
        c.turns.push(Turn {
            role: Role::Assistant,
            blocks: vec![
                Block::Text { text: "checking".into() },
                Block::ToolUse {
                    id: "t1".into(),
                    name: "bash".into(),
                    input: serde_json::json!({"command": "ls"}),
                },
                Block::ToolResult {
                    id: "t1".into(),
                    content: "file1\nfile2".into(),
                    is_error: false,
                },
                Block::Text { text: "done".into() },
            ],
        });
        let s = serialize(&c);
        let back = parse(&s).expect("parse");
        assert_eq!(back.turns.len(), 2);
        assert_eq!(back.turns[1].blocks.len(), 4);
        match &back.turns[1].blocks[1] {
            Block::ToolUse { name, .. } => assert_eq!(name, "bash"),
            _ => panic!("expected tool_use"),
        }
    }
}
