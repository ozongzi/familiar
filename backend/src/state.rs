use std::sync::Arc;

use crate::config::Config;
use crate::db::Db;
use agentix::Message;
use dashmap::DashMap;
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
    /// Pending Tauri OAuth tokens: state → session_token (short-lived, in-memory)
    pub pending_auth: Arc<DashMap<String, String>>,
}

const PUBLIC_PATH: &str = "/app/frontend/dist";
const ARTIFACTS_PATH: &str = "/app/artifacts";

impl AppState {
    pub fn new(_cfg: &Config, pool: PgPool) -> Self {
        let sandbox = Arc::new(crate::sandbox::SandboxManager::new(
            std::path::PathBuf::from(ARTIFACTS_PATH),
        ));
        let db = Db::new(pool.clone(), sandbox.clone());
        Self {
            public_path: PUBLIC_PATH.to_string(),
            artifacts_path: ARTIFACTS_PATH.to_string(),
            pool,
            db,
            sandbox,
            tunnel_registry: crate::web::tunnel::new_tunnel_registry(),
            github_client_id: std::env::var("GITHUB_CLIENT_ID").unwrap_or_default(),
            github_client_secret: std::env::var("GITHUB_CLIENT_SECRET").unwrap_or_default(),
            github_redirect_uri: std::env::var("GITHUB_REDIRECT_URI")
                .unwrap_or_else(|_| "http://localhost:5173/api/auth/github/callback".to_string()),
            pending_auth: Arc::new(DashMap::new()),
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
        // Use a transaction-scoped advisory lock keyed on the conversation UUID.
        // This prevents two concurrent requests from both passing the "no running
        // job" check and creating duplicate workers (TOCTOU race).
        let lock_key = i64::from_ne_bytes(conversation_id.as_bytes()[..8].try_into().unwrap());

        let mut tx = self.pool.begin().await?;

        sqlx::query("SELECT pg_advisory_xact_lock($1)")
            .bind(lock_key)
            .execute(&mut *tx)
            .await?;

        // Abort any active job inside the lock.
        let running: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM generation_jobs \
             WHERE conversation_id = $1 AND status IN ('pending', 'running') LIMIT 1",
        )
        .bind(conversation_id)
        .fetch_optional(&mut *tx)
        .await?;

        if let Some(existing) = running {
            sqlx::query(
                "UPDATE generation_jobs SET status = 'aborted', updated_at = now() \
                 WHERE id = $1 AND status IN ('pending', 'running')",
            )
            .bind(existing)
            .execute(&mut *tx)
            .await?;
        }

        let job_id: Uuid = sqlx::query_scalar(
            "INSERT INTO generation_jobs (conversation_id, user_id) VALUES ($1, $2) RETURNING id",
        )
        .bind(conversation_id)
        .bind(user_id)
        .fetch_one(&mut *tx)
        .await?;

        tx.commit().await?;

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

    /// Mark a job as 'interrupted': the interrupt handler has already persisted
    /// the partial reply, so the worker should exit cleanly without double-saving.
    pub async fn interrupt_job(&self, job_id: Uuid) {
        let _ = sqlx::query(
            "UPDATE generation_jobs SET status = 'interrupted', updated_at = now() WHERE id = $1 AND status IN ('pending', 'running')",
        )
        .bind(job_id)
        .execute(&self.pool)
        .await;
    }

    /// Persist a message (fire-and-forget with embedding).
    pub fn persist_message(&self, conversation_id: Uuid, user_id: Uuid, msg: &Message) {
        let state = self.clone();
        let msg = msg.clone();
        tokio::spawn(async move {
            state.persist_message_async(conversation_id, user_id, msg).await;
        });
    }

    pub async fn persist_message_async(
        &self,
        conversation_id: Uuid,
        user_id: Uuid,
        msg: Message,
    ) -> Option<i64> {
        use crate::db::to_vector;
        use crate::embedding::EmbeddingClient;
        use agentix::UserContent;

        let db = self.db.clone();

        let text_for_embed: Option<String> = match &msg {
            Message::User(parts) => {
                let t: String = parts
                    .iter()
                    .filter_map(|p| match p {
                        UserContent::Text { text: s } => Some(s.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("");
                if t.is_empty() { None } else { Some(t) }
            }
            Message::Assistant {
                content: Some(c), ..
            } if !c.is_empty() => Some(c.clone()),
            _ => None,
        };

        let row_id = match db.append(conversation_id, user_id, &msg, None).await {
            Ok(id) => id,
            Err(e) => {
                tracing::error!("db append failed: {e}");
                return None;
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

        Some(row_id)
    }
}
