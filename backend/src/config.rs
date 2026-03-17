use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::PgPool;
use std::collections::HashMap;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Config {
    pub public_path: String,
    pub artifacts_path: String,
    pub frontier_model: ModelConfig,
    pub cheap_model: ModelConfig,
    pub embedding: ModelConfig,
    pub server: ServerConfig,
    #[serde(default)]
    pub mcp: Vec<McpServerConfig>,
    #[serde(default)]
    pub mcp_catalog: Vec<McpCatalogEntry>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ModelConfig {
    pub api_key: String,
    pub api_base: String,
    pub name: String,
    #[serde(default)]
    pub extra_body: HashMap<String, Value>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ServerConfig {
    pub port: u16,
    pub system_prompt: Option<String>,
    pub subagent_prompt: Option<String>,
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

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct McpCatalogEntry {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Debug, Deserialize, sqlx::FromRow)]
struct AppConfigRow {
    config_json: Value,
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

impl Default for Config {
    fn default() -> Self {
        let default_model = ModelConfig {
            api_key: String::new(),
            api_base: "https://api.deepseek.com/v1".to_string(),
            name: "deepseek-chat".to_string(),
            extra_body: HashMap::new(),
        };

        Self {
            public_path: "/srv/familiar/frontend/dist".to_string(),
            artifacts_path: "/root/workplace/artifacts".to_string(),
            frontier_model: default_model.clone(),
            cheap_model: default_model.clone(),
            embedding: default_model,
            server: ServerConfig {
                port: 3000,
                system_prompt: None,
                subagent_prompt: None,
            },
            mcp: vec![],
            mcp_catalog: vec![],
        }
    }
}

impl Config {
    pub async fn load_from_db(pool: &PgPool) -> anyhow::Result<Self> {
        let row =
            sqlx::query_as::<_, AppConfigRow>("SELECT config_json FROM app_config WHERE id = true")
                .fetch_optional(pool)
                .await?;

        if let Some(row) = row {
            let cfg: Config = serde_json::from_value(row.config_json)?;
            Ok(cfg)
        } else {
            Ok(Self::default())
        }
    }

    pub async fn upsert(pool: &PgPool, cfg: &Config) -> anyhow::Result<()> {
        let payload = serde_json::to_value(cfg)?;
        sqlx::query(
            r#"
            INSERT INTO app_config (id, config_json, updated_at)
            VALUES (true, $1, NOW())
            ON CONFLICT (id) DO UPDATE SET
                config_json = EXCLUDED.config_json,
                updated_at = NOW()
            "#,
        )
        .bind(payload)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub fn system_prompt(&self) -> Option<String> {
        self.server.system_prompt.clone()
    }

    pub(crate) fn subagent_prompt(&self) -> Option<String> {
        self.server.subagent_prompt.clone()
    }
}
