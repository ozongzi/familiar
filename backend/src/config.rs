use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::PgPool;
use std::collections::HashMap;
use uuid::Uuid;

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
pub struct McpCatalogEntry {
    pub name: String,
    pub description: String,
    pub command: String,
    pub args: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub enum Provider {
    #[serde(rename = "deepseek")]
    DeepSeek,
    #[serde(rename = "openai")]
    OpenAI,
    #[serde(rename = "anthropic")]
    Anthropic,
    #[serde(rename = "gemini")]
    Gemini,
}

impl Default for Provider {
    fn default() -> Self {
        Provider::DeepSeek
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ModelConfig {
    pub api_key: String,
    pub api_base: String,
    pub name: String,
    #[serde(default)]
    pub provider: Provider,
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
    public_path: Option<String>,
    artifacts_path: Option<String>,
    frontier_model: Option<Value>,
    cheap_model: Option<Value>,
    embedding_model: Option<Value>,
    server_port: Option<i32>,
    system_prompt: Option<String>,
    subagent_prompt: Option<String>,
    mcp_catalog: Option<Value>,
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
            provider: Provider::DeepSeek,
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
        let row = sqlx::query_as::<_, AppConfigRow>(
            r#"
            SELECT 
                public_path, artifacts_path, frontier_model, cheap_model, embedding_model, 
                server_port, system_prompt, subagent_prompt, mcp_catalog 
            FROM app_config WHERE id = true
            "#,
        )
        .fetch_optional(pool)
        .await?;

        let mut cfg = Self::default();

        if let Some(r) = row {
            if let Some(pp) = r.public_path { cfg.public_path = pp; }
            if let Some(ap) = r.artifacts_path { cfg.artifacts_path = ap; }
            if let Some(fm) = r.frontier_model { cfg.frontier_model = serde_json::from_value(fm).unwrap_or(cfg.frontier_model); }
            if let Some(cm) = r.cheap_model { cfg.cheap_model = serde_json::from_value(cm).unwrap_or(cfg.cheap_model); }
            if let Some(em) = r.embedding_model { cfg.embedding = serde_json::from_value(em).unwrap_or(cfg.embedding); }
            
            if let Some(port) = r.server_port { cfg.server.port = port as u16; }
            cfg.server.system_prompt = r.system_prompt;
            cfg.server.subagent_prompt = r.subagent_prompt;
            
            if let Some(mc) = r.mcp_catalog { cfg.mcp_catalog = serde_json::from_value(mc).unwrap_or(cfg.mcp_catalog); }
        }

        // Load Global MCPs
        let mcps = sqlx::query_as::<_, GlobalMcp>("SELECT * FROM global_mcps ORDER BY created_at ASC")
            .fetch_all(pool)
            .await?;
        
        cfg.mcp = mcps.into_iter().map(|m| {
            match m.r#type.as_str() {
                "http" => {
                    let url = m.config.get("url").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    McpServerConfig::Http { name: m.name, url }
                },
                "stdio" => {
                    let command = m.config.get("command").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let args = m.config.get("args").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
                    let env = m.config.get("env").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
                    McpServerConfig::Studio { name: m.name, command, args, env }
                },
                _ => McpServerConfig::Http { name: m.name, url: "".to_string() }, // Fallback
            }
        }).collect();

        Ok(cfg)
    }

    pub async fn upsert(pool: &PgPool, cfg: &Config) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO app_config (
                id, public_path, artifacts_path, frontier_model, cheap_model, embedding_model, 
                server_port, system_prompt, subagent_prompt, mcp_catalog, updated_at
            )
            VALUES (true, $1, $2, $3, $4, $5, $6, $7, $8, $9, NOW())
            ON CONFLICT (id) DO UPDATE SET
                public_path = EXCLUDED.public_path,
                artifacts_path = EXCLUDED.artifacts_path,
                frontier_model = EXCLUDED.frontier_model,
                cheap_model = EXCLUDED.cheap_model,
                embedding_model = EXCLUDED.embedding_model,
                server_port = EXCLUDED.server_port,
                system_prompt = EXCLUDED.system_prompt,
                subagent_prompt = EXCLUDED.subagent_prompt,
                mcp_catalog = EXCLUDED.mcp_catalog,
                updated_at = NOW()
            "#,
        )
        .bind(&cfg.public_path)
        .bind(&cfg.artifacts_path)
        .bind(serde_json::to_value(&cfg.frontier_model)?)
        .bind(serde_json::to_value(&cfg.cheap_model)?)
        .bind(serde_json::to_value(&cfg.embedding)?)
        .bind(cfg.server.port as i32)
        .bind(&cfg.server.system_prompt)
        .bind(&cfg.server.subagent_prompt)
        .bind(serde_json::to_value(&cfg.mcp_catalog)?)
        .execute(pool)
        .await?;
        
        // Note: Global MCPs are NOT updated here. Use dedicated endpoints.
        
        Ok(())
    }

    pub fn system_prompt(&self) -> Option<String> {
        self.server.system_prompt.clone()
    }

    pub(crate) fn subagent_prompt(&self) -> Option<String> {
        self.server.subagent_prompt.clone()
    }
}
