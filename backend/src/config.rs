use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::PgPool;
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Config {
    pub cheap_model: ModelConfig,
    pub embedding: ModelConfig,
    #[serde(default)]
    pub mcp: Vec<McpServerConfig>,
    #[serde(default)]
    pub mcp_catalog: Vec<McpCatalogEntry>,
    pub tavily_api_key: Option<String>,
    pub siliconflow_api_key: Option<String>,
    pub fal_api_key: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct McpCatalogEntry {
    pub name: String,
    pub description: String,
    pub command: String,
    pub args: Vec<String>,
}

pub use agentix::Provider;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ModelConfig {
    pub api_key: String,
    pub api_base: String,
    pub name: String,
    pub provider: Provider,
    #[serde(default)]
    pub extra_body: HashMap<String, Value>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
    /// Backend dispatch kind: "api" (default) → HTTP provider via `to_request`,
    /// "claude-code" → subprocess-backed `Provider::ClaudeCode` via `Request`.
    #[serde(default = "default_model_kind")]
    pub kind: String,
    /// Input-context size snapshot (`messages.context_tokens` on the latest
    /// assistant turn) at which the worker triggers a compaction pass using
    /// this model.
    #[serde(default = "default_compact_trigger")]
    pub compact_trigger_tokens: i64,
    /// Tokens of recent history kept raw after a compaction (the live tail).
    #[serde(default = "default_compact_tail")]
    pub compact_tail_tokens: i64,
}

fn default_compact_trigger() -> i64 {
    50_000
}
fn default_compact_tail() -> i64 {
    16_000
}

fn default_model_kind() -> String {
    "api".to_string()
}

impl ModelConfig {
    /// Build an [`agentix::Request`] pre-filled with provider, key, base URL, and model.
    pub fn to_request(&self) -> agentix::Request {
        let provider = if self.kind == "claude-code" {
            Provider::ClaudeCode
        } else {
            self.provider
        };
        let req = agentix::Request::new(provider, &self.api_key)
            .base_url(&self.api_base)
            .model(&self.name);
        if let Some(mt) = self.max_tokens {
            req.max_tokens(mt)
        } else {
            req
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(untagged)]
pub enum McpServerConfig {
    Studio {
        name: String,
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: HashMap<String, String>,
    },
    Http {
        name: String,
        url: String,
    },
}

#[derive(Debug, Deserialize, Serialize, Clone, sqlx::FromRow)]
pub struct GlobalMcp {
    pub id: Uuid,
    pub name: String,
    pub r#type: String,
    pub config: Value,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Deserialize, sqlx::FromRow)]
struct AppConfigRow {
    tavily_api_key: Option<String>,
    siliconflow_api_key: Option<String>,
    fal_api_key: Option<String>,
}

#[derive(Debug, Clone)]
pub struct EnvConfig {
    pub database_url: String,
}

impl EnvConfig {
    pub fn load() -> Self {
        let database_url =
            std::env::var("DATABASE_URL").unwrap_or_else(|_| panic!("DATABASE_URL is required"));
        Self { database_url }
    }
}

fn default_model() -> ModelConfig {
    ModelConfig {
        api_key: String::new(),
        api_base: "https://api.deepseek.com/v1".to_string(),
        name: "deepseek-chat".to_string(),
        provider: Provider::DeepSeek,
        extra_body: HashMap::new(),
        max_tokens: None,
        kind: "api".to_string(),
        compact_trigger_tokens: default_compact_trigger(),
        compact_tail_tokens: default_compact_tail(),
    }
}

impl Default for Config {
    fn default() -> Self {
        let m = default_model();
        Self {
            cheap_model: m.clone(),
            embedding: m,
            mcp: vec![],
            mcp_catalog: vec![],
            tavily_api_key: None,
            siliconflow_api_key: None,
            fal_api_key: None,
        }
    }
}

impl Config {
    pub async fn load_from_db(pool: &PgPool) -> anyhow::Result<Self> {
        let mut cfg = Self::default();

        // ── Load API keys + mcp_catalog from app_config ───────────────────
        let row = sqlx::query_as::<_, AppConfigRow>(
            "SELECT tavily_api_key, siliconflow_api_key, fal_api_key
             FROM app_config WHERE id = true",
        )
        .fetch_optional(pool)
        .await?;

        if let Some(r) = row {
            cfg.tavily_api_key = r.tavily_api_key;
            cfg.siliconflow_api_key = r.siliconflow_api_key;
            cfg.fal_api_key = r.fal_api_key;
        }

        // ── Load MCP catalog from dedicated table ─────────────────────────
        #[derive(sqlx::FromRow)]
        struct CatalogRow {
            name: String,
            description: String,
            command: String,
            args: Value,
        }
        let catalog_rows = sqlx::query_as::<_, CatalogRow>(
            "SELECT name, description, command, args FROM mcp_catalog ORDER BY created_at ASC",
        )
        .fetch_all(pool)
        .await?;
        cfg.mcp_catalog = catalog_rows
            .into_iter()
            .map(|r| McpCatalogEntry {
                name: r.name,
                description: r.description,
                command: r.command,
                args: serde_json::from_value(r.args).unwrap_or_default(),
            })
            .collect();

        // ── Load cheap model from models table ────────────────────────────
        let cheap: Option<(String, String, String, String, Value, i64, i64)> = sqlx::query_as(
            "SELECT provider, model_name, api_base, api_key, extra_body,
                    compact_trigger_tokens, compact_tail_tokens
             FROM models WHERE scope = 'global' AND role = 'cheap' LIMIT 1",
        )
        .fetch_optional(pool)
        .await?;

        if let Some((provider, name, api_base, api_key, extra_body, trig, tail)) = cheap {
            cfg.cheap_model = ModelConfig {
                provider: serde_json::from_value(Value::String(provider))
                    .unwrap_or(Provider::DeepSeek),
                name,
                api_base,
                api_key,
                extra_body: extra_body
                    .as_object()
                    .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                    .unwrap_or_default(),
                max_tokens: None,
                kind: "api".to_string(),
                compact_trigger_tokens: trig,
                compact_tail_tokens: tail,
            };
        }

        // ── Load embedding model from models table ────────────────────────
        let embed: Option<(String, String, String, String, Value, i64, i64)> = sqlx::query_as(
            "SELECT provider, model_name, api_base, api_key, extra_body,
                    compact_trigger_tokens, compact_tail_tokens
             FROM models WHERE scope = 'global' AND role = 'embedding' LIMIT 1",
        )
        .fetch_optional(pool)
        .await?;

        if let Some((provider, name, api_base, api_key, extra_body, trig, tail)) = embed {
            cfg.embedding = ModelConfig {
                provider: serde_json::from_value(Value::String(provider))
                    .unwrap_or(Provider::DeepSeek),
                name,
                api_base,
                api_key,
                extra_body: extra_body
                    .as_object()
                    .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                    .unwrap_or_default(),
                max_tokens: None,
                kind: "api".to_string(),
                compact_trigger_tokens: trig,
                compact_tail_tokens: tail,
            };
        }

        // ── Load Global MCPs ──────────────────────────────────────────────
        let mcps =
            sqlx::query_as::<_, GlobalMcp>("SELECT * FROM global_mcps ORDER BY created_at ASC")
                .fetch_all(pool)
                .await?;

        cfg.mcp = mcps
            .into_iter()
            .map(|m| match m.r#type.as_str() {
                "http" => {
                    let url = m
                        .config
                        .get("url")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    McpServerConfig::Http { name: m.name, url }
                }
                "stdio" => {
                    let command = m
                        .config
                        .get("command")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let args = m
                        .config
                        .get("args")
                        .and_then(|v| serde_json::from_value(v.clone()).ok())
                        .unwrap_or_default();
                    let env = m
                        .config
                        .get("env")
                        .and_then(|v| serde_json::from_value(v.clone()).ok())
                        .unwrap_or_default();
                    McpServerConfig::Studio {
                        name: m.name,
                        command,
                        args,
                        env,
                    }
                }
                _ => McpServerConfig::Http {
                    name: m.name,
                    url: "".to_string(),
                },
            })
            .collect();

        Ok(cfg)
    }

    pub async fn upsert(pool: &PgPool, cfg: &Config) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO app_config (id, tavily_api_key, siliconflow_api_key, fal_api_key, updated_at)
            VALUES (true, $1, $2, $3, NOW())
            ON CONFLICT (id) DO UPDATE SET
                tavily_api_key      = EXCLUDED.tavily_api_key,
                siliconflow_api_key = EXCLUDED.siliconflow_api_key,
                fal_api_key         = EXCLUDED.fal_api_key,
                updated_at          = NOW()
            "#,
        )
        .bind(&cfg.tavily_api_key)
        .bind(&cfg.siliconflow_api_key)
        .bind(&cfg.fal_api_key)
        .execute(pool)
        .await?;

        // Note: Global MCPs and MCP catalog have dedicated endpoints.
        // cheap_model and embedding are stored in the models table (role='cheap'/'embedding').

        Ok(())
    }
}
