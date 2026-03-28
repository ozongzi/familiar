use std::sync::Arc;

use crate::config::Config;
use crate::db::Db;
use agentix::Message;
use sqlx::PgPool;
use uuid::Uuid;

/// Slim, stateless application state. All conversation state lives in the DB.
/// The only in-memory piece is the tunnel registry (live WebSocket handles).
#[allow(unused)]
#[derive(Clone)]
pub struct AppState {
    pub public_path: String,
    pub artifacts_path: String,
    pub pool: PgPool,
    pub db: Db,
    pub sandbox: Arc<crate::sandbox::SandboxManager>,
    pub tunnel_registry: crate::web::tunnel::TunnelRegistry,
    pub github_client_id: String,
    pub github_client_secret: String,
    pub github_redirect_uri: String,
}

impl AppState {
    pub fn new(cfg: &Config, pool: PgPool) -> Self {
        let db = Db::new(pool.clone());
        let sandbox = Arc::new(crate::sandbox::SandboxManager::new(
            std::path::PathBuf::from(&cfg.artifacts_path),
        ));
        Self {
            public_path: cfg.public_path.clone(),
            artifacts_path: cfg.artifacts_path.clone(),
            pool,
            db,
            sandbox,
            tunnel_registry: crate::web::tunnel::new_tunnel_registry(),
            github_client_id: std::env::var("GITHUB_CLIENT_ID").unwrap_or_default(),
            github_client_secret: std::env::var("GITHUB_CLIENT_SECRET").unwrap_or_default(),
            github_redirect_uri: std::env::var("GITHUB_REDIRECT_URI")
                .unwrap_or_else(|_| "http://localhost:5173/api/auth/github/callback".to_string()),
        }
    }

    pub async fn get_global_config(&self) -> crate::errors::AppResult<Config> {
        Config::load_from_db(&self.pool)
            .await
            .map_err(|e| crate::errors::AppError::internal(&format!("无法加载全局配置: {}", e)))
    }

    // ── Job management (DB-driven) ────────────────────────────────────────

    /// Create a generation job and spawn a background worker.
    /// Returns the job_id (which doubles as the stream_id for SSE).
    pub async fn start_generation(
        &self,
        conversation_id: Uuid,
        user_id: Uuid,
    ) -> anyhow::Result<Uuid> {
        // Check if there's already a running job for this conversation.
        let running: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM generation_jobs WHERE conversation_id = $1 AND status IN ('pending', 'running') LIMIT 1",
        )
        .bind(conversation_id)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(existing) = running {
            // Abort the existing job, then start a new one.
            self.abort_job(existing).await;
        }

        let job_id: Uuid = sqlx::query_scalar(
            "INSERT INTO generation_jobs (conversation_id, user_id) VALUES ($1, $2) RETURNING id",
        )
        .bind(conversation_id)
        .bind(user_id)
        .fetch_one(&self.pool)
        .await?;

        let ctx = crate::worker::WorkerContext {
            job_id,
            conversation_id,
            user_id,
            pool: self.pool.clone(),
            db: self.db.clone(),
            sandbox: self.sandbox.clone(),
            tunnel_registry: self.tunnel_registry.clone(),
        };
        crate::worker::spawn_worker(ctx);

        Ok(job_id)
    }

    /// Abort a running generation job.
    pub async fn abort_job(&self, job_id: Uuid) {
        let _ = sqlx::query(
            "UPDATE generation_jobs SET status = 'aborted', updated_at = now() WHERE id = $1 AND status IN ('pending', 'running')",
        )
        .bind(job_id)
        .execute(&self.pool)
        .await;
    }

    /// Persist a message (fire-and-forget with embedding).
    pub fn persist_message(&self, conversation_id: Uuid, msg: &Message) {
        let state = self.clone();
        let msg = msg.clone();
        tokio::spawn(async move {
            state.persist_message_async(conversation_id, msg).await;
        });
    }

    pub async fn persist_message_async(&self, conversation_id: Uuid, msg: Message) {
        use agentix::UserContent;
        use crate::embedding::EmbeddingClient;
        use crate::db::to_vector;

        let db = self.db.clone();

        let text_for_embed: Option<String> = match &msg {
            Message::User(parts) => {
                let t: String = parts
                    .iter()
                    .filter_map(|p| match p {
                        UserContent::Text(s) => Some(s.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("");
                if t.is_empty() { None } else { Some(t) }
            }
            Message::Assistant { content: Some(c), .. } if !c.is_empty() => Some(c.clone()),
            _ => None,
        };

        let row_id = match db.append(conversation_id, &msg, None).await {
            Ok(id) => id,
            Err(e) => {
                tracing::error!("db append failed: {e}");
                return;
            }
        };

        if let Some(content) = text_for_embed {
            let pool = self.pool.clone();
            tokio::spawn(async move {
                let global_cfg = Config::load_from_db(&pool).await.unwrap_or_default();
                let embed = EmbeddingClient::new(
                    global_cfg.embedding.api_key,
                    global_cfg.embedding.api_base,
                    global_cfg.embedding.name,
                );
                match embed.embed(&content).await {
                    Ok(vec) => {
                        let vector = to_vector(vec);
                        if let Err(e) = db.set_embedding(row_id, vector).await {
                            tracing::error!("set_embedding failed: {e}");
                        }
                    }
                    Err(e) => tracing::error!("embed failed: {e}"),
                }
            });
        }
    }
}
