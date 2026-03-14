use config::{Config as Cfg, Environment, File};
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;

/// Top-level configuration for familiar.
///
/// Loading order (later sources override earlier ones):
///   1. `config.toml`  — all settings including secrets, git-ignored
///   2. Environment variables prefixed with `FAMILIAR__`
///      e.g. `FAMILIAR__SECRETS__DEEPSEEK_API_KEY=sk-...`
///      `FAMILIAR__SERVER__PORT=8080`
#[derive(Debug, Deserialize)]
pub struct Config {
    pub public_path: String,
    pub artifacts_path: String,
    pub secrets: Secrets,
    pub model: ModelConfig,
    pub embedding: ModelConfig,
    pub server: ServerConfig,
    #[serde(default)]
    pub mcp: Vec<McpServerConfig>,
    /// Catalogue of MCPs that the agent can install on demand.
    #[serde(default)]
    #[allow(dead_code)]
    pub mcp_catalog: Vec<McpCatalogEntry>,
}

/// Sensitive credentials.
#[derive(Debug, Deserialize)]
pub struct Secrets {
    pub database_url: String,
}

/// LLM or embedding model configuration.
#[derive(Debug, Deserialize)]
pub struct ModelConfig {
    pub api_key: String,
    pub api_base: String,
    pub name: String,
    /// Optional arbitrary body fields to include when sending model requests.
    /// Loaded from `extra_body` in the config file (table of key = value).
    #[serde(default)]
    pub extra_body: HashMap<String, Value>,
}

/// HTTP server configuration.
#[derive(Debug, Deserialize)]
pub struct ServerConfig {
    pub port: u16,
    /// Path to a file whose contents become the system prompt.
    pub system_prompt: Option<String>,
    pub subagent_prompt: Option<String>,
}

/// A single MCP server to launch at startup.
#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
pub enum McpServerConfig {
    Studio {
        name: String,
        command: String,
        #[serde(default)]
        args: Vec<String>,
        /// Environment variables to inject into the MCP subprocess.
        #[serde(default)]
        env: HashMap<String, String>,
    },
    Http {
        name: String,
        url: String,
    }
}

/// A catalogued MCP server available for on-demand installation by the agent.
#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct McpCatalogEntry {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
}


impl Config {
    pub fn load() -> Self {
        let config_path =
            std::env::var("FAMILIAR_CONFIG").unwrap_or("/srv/familiar/config.toml".into());

        let cfg = Cfg::builder()
            .add_source(File::with_name(&config_path).required(true))
            .add_source(
                Environment::with_prefix("FAMILIAR")
                    .separator("__")
                    .try_parsing(true),
            )
            .build()
            .expect("failed to build configuration");

        cfg.try_deserialize().expect("invalid configuration")
    }

    /// Returns the system prompt. Falls back to a built-in default if none is configured.
    /// Appends skills summary if any skills are found in /srv/familiar/skills/.
    pub fn system_prompt(&self) -> Option<String> {
        let base = self.server.system_prompt.clone();
        if let Some(summary) = Self::skills_summary() {
            let base = base.unwrap_or_default();
            Some(format!("{base}{summary}"))
        } else {
            base
        }
    }

    /// Scans /srv/familiar/skills/ and returns a summary string listing
    /// available skills (name + description from frontmatter), or None if
    /// the directory is empty or missing.
    pub fn skills_summary() -> Option<String> {
        let dir = std::path::Path::new("/srv/familiar/skills");
        let entries = std::fs::read_dir(dir).ok()?;
        let mut skills = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            let content = std::fs::read_to_string(&path).unwrap_or_default();
            let (name, description) = parse_skill_meta(&content);
            let name = name.unwrap_or_else(|| {
                path.file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown")
                    .to_string()
            });
            if let Some(desc) = description {
                skills.push(format!("- {name}: {desc}"));
            } else {
                skills.push(format!("- {name}"));
            }
        }
        if skills.is_empty() {
            return None;
        }
        skills.sort();
        Some(format!(
            "\n\n可用 Skills（需要时调用 load_skill 获取详细指令）：\n{}",
            skills.join("\n")
        ))
    }

    pub(crate) fn subagent_prompt(&self) -> Option<String> {
        self.server.subagent_prompt.clone()
    }
}

/// Extract `name` and `description` from YAML frontmatter of a skill file.
fn parse_skill_meta(content: &str) -> (Option<String>, Option<String>) {
    let content = content.trim_start();
    if !content.starts_with("---") {
        return (None, None);
    }
    let inner = &content[3..];
    let end = match inner.find("\n---") {
        Some(i) => i,
        None => return (None, None),
    };
    let frontmatter = &inner[..end];
    let mut name = None;
    let mut description = None;
    for line in frontmatter.lines() {
        if let Some(v) = line.strip_prefix("name:") {
            name = Some(v.trim().to_string());
        } else if let Some(v) = line.strip_prefix("description:") {
            description = Some(v.trim().to_string());
        }
    }
    (name, description)
}