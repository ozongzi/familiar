use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::paths;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub anthropic_api_key: String,
    pub model: String,
    pub max_tokens: u32,
    pub system_prompt: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            anthropic_api_key: String::new(),
            model: "claude-sonnet-4-6".to_string(),
            max_tokens: 8192,
            system_prompt: default_system_prompt(),
        }
    }
}

pub fn default_system_prompt() -> String {
    r#"You are Familiar, a local desktop AI assistant.

You have access to two tools:
- `bash`: run a shell command in the conversation's workspace directory.
- `read_file` / `write_file`: inspect and create files in the workspace.

The workspace is a real directory on the user's machine. There is no sandbox.
Be careful with destructive commands. Confirm before deleting anything the user
didn't explicitly ask for. Prefer reading before writing. Keep responses concise."#
        .to_string()
    }

impl Config {
    pub fn load() -> Result<Self> {
        let path = paths::config_path();
        if !path.exists() {
            let cfg = Self::default();
            cfg.save()?;
            return Ok(cfg);
        }
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("read {}", path.display()))?;
        let cfg: Self = toml::from_str(&raw).context("parse config.toml")?;
        Ok(cfg)
    }

    pub fn save(&self) -> Result<()> {
        let path = paths::config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let raw = toml::to_string_pretty(self).context("serialize config")?;
        std::fs::write(&path, raw).with_context(|| format!("write {}", path.display()))?;
        Ok(())
    }
}
