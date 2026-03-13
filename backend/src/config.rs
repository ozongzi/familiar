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
}

/// A single MCP server to launch at startup.
#[derive(Debug, Deserialize, Clone)]
pub struct McpServerConfig {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// Environment variables to inject into the MCP subprocess.
    #[serde(default)]
    pub env: HashMap<String, String>,
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

    /// Read the system prompt from disk if `server.system_prompt_file` is set.
    pub fn system_prompt(&self) -> Option<String> {
        self.server.system_prompt.clone()
    }
}
