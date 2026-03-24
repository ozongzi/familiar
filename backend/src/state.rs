use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::config::{Config, McpServerConfig, ModelConfig};
use crate::db::{Db, to_vector};
use crate::embedding::EmbeddingClient;
use crate::spells::{SpellDeps, build_all_spells};
use agentix::{Agent, AgentEvent, InMemory, McpTool, Message};
use agentix::types::UsageStats;
use futures::StreamExt;
use serde_json::{json};
use sqlx::PgPool;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::broadcast;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

const EVENT_LOG_CAP: usize = 4096;
const BROADCAST_CAP: usize = 256;

static NEXT_SEQ: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone)]
pub struct WsEvent {
    pub seq: u64,
    pub payload: String,
}

pub struct ChatEntry {
    pub user_id: Uuid,
    pub agent: Arc<tokio::sync::Mutex<Agent>>,
    pub user_mcp_tools: Arc<tokio::sync::Mutex<Vec<(String, McpTool)>>>,
    pub broadcast_tx: broadcast::Sender<Arc<WsEvent>>,
    pub event_log: VecDeque<Arc<WsEvent>>,
    pub generating: bool,
    pub abort_flag: Arc<AtomicBool>,
    pub ask_user_pending: Arc<tokio::sync::Mutex<Option<tokio::sync::oneshot::Sender<String>>>>,
    pub queued_interrupts: Vec<String>,
    #[allow(dead_code)]
    pub spawn_tx: tokio::sync::broadcast::Sender<String>,
}

impl ChatEntry {
    fn new(
        user_id: Uuid,
        agent: Arc<tokio::sync::Mutex<Agent>>,
        spawn_tx: tokio::sync::broadcast::Sender<String>,
        user_mcp_tools: Arc<tokio::sync::Mutex<Vec<(String, McpTool)>>>,
        ask_user_pending: Arc<tokio::sync::Mutex<Option<tokio::sync::oneshot::Sender<String>>>>,
        abort_flag: Arc<AtomicBool>,
    ) -> Self {
        let (broadcast_tx, _) = broadcast::channel(BROADCAST_CAP);
        Self {
            user_id,
            agent,
            user_mcp_tools,
            broadcast_tx,
            event_log: VecDeque::new(),
            generating: false,
            abort_flag,
            ask_user_pending,
            queued_interrupts: Vec::new(),
            spawn_tx,
        }
    }

    pub fn emit(&mut self, payload: String) {
        let seq = NEXT_SEQ.fetch_add(1, AtomicOrdering::Relaxed);
        let ev = Arc::new(WsEvent { seq, payload });
        if self.event_log.len() >= EVENT_LOG_CAP {
            self.event_log.pop_front();
        }
        self.event_log.push_back(Arc::clone(&ev));
        let _ = self.broadcast_tx.send(ev);
    }

    pub fn clear_log(&mut self) {
        self.event_log.clear();
    }
}

#[allow(unused)]
#[derive(Clone)]
pub struct AppState {
    pub chats: Arc<Mutex<HashMap<Uuid, ChatEntry>>>,
    pub public_path: String,
    pub artifacts_path: String,
    pub streams: Arc<Mutex<HashMap<Uuid, (Uuid, Uuid)>>>,
    pub pool: PgPool,
    pub db: Db,
    pub sandbox: Arc<crate::sandbox::SandboxManager>,
    pub mcp_tools: Arc<tokio::sync::Mutex<Vec<(String, McpTool)>>>,
    pub tunnel_registry: crate::web::tunnel::TunnelRegistry,
    pub github_client_id: String,
    pub github_client_secret: String,
    pub github_redirect_uri: String,
}

impl AppState {
    pub fn new(cfg: &Config, pool: PgPool, mcp_tools: Vec<(String, McpTool)>) -> Self {
        let db = Db::new(pool.clone());
        let sandbox = Arc::new(crate::sandbox::SandboxManager::new(
            std::path::PathBuf::from(&cfg.artifacts_path),
        ));
        Self {
            chats: Arc::new(Mutex::new(HashMap::new())),
            streams: Arc::new(Mutex::new(HashMap::new())),
            public_path: cfg.public_path.clone(),
            artifacts_path: cfg.artifacts_path.clone(),
            pool,
            db,
            sandbox,
            mcp_tools: Arc::new(tokio::sync::Mutex::new(mcp_tools)),
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

    pub fn create_stream(&self, conversation_id: Uuid, user_id: Uuid) -> Uuid {
        let stream_id = Uuid::new_v4();
        self.streams
            .lock()
            .unwrap()
            .insert(stream_id, (conversation_id, user_id));
        stream_id
    }

    pub fn resolve_stream(&self, stream_id: Uuid) -> Option<(Uuid, Uuid)> {
        self.streams.lock().unwrap().get(&stream_id).copied()
    }

    pub async fn init_mcp(mcp_configs: &[McpServerConfig]) -> Vec<(String, McpTool)> {
        let mut tools = Vec::new();
        for mc in mcp_configs {
            match mc {
                McpServerConfig::Studio {
                    name,
                    command,
                    args,
                    env,
                } => {
                    let env_args: Vec<String>;
                    let (cmd, args): (&str, Vec<&str>) = if env.is_empty() {
                        (command, args.iter().map(String::as_str).collect())
                    } else {
                        env_args = env
                            .iter()
                            .map(|(k, v)| format!("{k}={v}"))
                            .chain(std::iter::once(command.clone()))
                            .chain(args.iter().cloned())
                            .collect();
                        ("env", env_args.iter().map(String::as_str).collect())
                    };
                    match McpTool::stdio(cmd, &args).await {
                        Ok(t) => {
                            info!("MCP: {} ready ({} tools)", name, t.raw_tools().len());
                            tools.push((name.clone(), t));
                        }
                        Err(e) => warn!("MCP: {} failed to start: {e}", name),
                    }
                }
                McpServerConfig::Http { name, url } => match McpTool::http(url).await {
                    Ok(t) => {
                        info!("MCP: {} ready ({} tools)", name, t.raw_tools().len());
                        tools.push((name.clone(), t));
                    }
                    Err(e) => warn!("MCP: {} failed to start: {e}", name),
                },
            }
        }
        tools
    }

    pub async fn build_agent(&self, conversation_id: Uuid) -> Arc<tokio::sync::Mutex<Agent>> {
        let global_cfg = self.get_global_config().await.unwrap_or_default();

        let history = match self.db.restore(conversation_id).await {
            Ok(h) => {
                info!(conversation = %conversation_id, messages = h.len(), "restored history");
                h
            }
            Err(e) => {
                error!(conversation = %conversation_id, "failed to restore history: {e}");
                vec![]
            }
        };

        let mcp_snapshot: Vec<_> = {
            let guard = self.mcp_tools.lock().await;
            guard.iter().cloned().collect()
        };

        let user_id: Uuid =
            sqlx::query_scalar::<_, Uuid>("SELECT user_id FROM conversations WHERE id = $1")
                .bind(conversation_id)
                .fetch_optional(&self.pool)
                .await
                .unwrap_or(None)
                .unwrap_or_else(|| {
                    warn!(conversation = %conversation_id, "conversation not found, using nil user_id");
                    Uuid::nil()
                });

        let user_name: String = sqlx::query_scalar::<_, String>("SELECT name FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_optional(&self.pool)
            .await
            .unwrap_or(None)
            .unwrap_or_default();

        let user_mcp_rows: Vec<(String, String, serde_json::Value)> = sqlx::query_as(
            r#"SELECT name, "type", config FROM user_mcps WHERE user_id = $1
               UNION ALL
               SELECT name, "type", config FROM conversation_mcps WHERE conversation_id = $2
               ORDER BY name ASC"#
        )
        .bind(user_id)
        .bind(conversation_id)
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default();

        let (spawn_tx, _) = tokio::sync::broadcast::channel::<String>(256);

        let user_settings: Option<(Option<serde_json::Value>, Option<serde_json::Value>, Option<String>)> = sqlx::query_as(
            "SELECT frontier_model, cheap_model, system_prompt FROM user_settings WHERE user_id = $1"
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
        .unwrap_or(None);

        let (frontier_cfg, cheap_cfg, mut system_prompt) = if let Some((f, c, p)) = user_settings {
            let f_cfg: ModelConfig = f
                .and_then(|v| serde_json::from_value(v).ok())
                .unwrap_or_else(|| global_cfg.frontier_model.clone());
            let c_cfg: ModelConfig = c
                .and_then(|v| serde_json::from_value(v).ok())
                .unwrap_or_else(|| global_cfg.cheap_model.clone());
            let s_prompt = p.or_else(|| global_cfg.system_prompt());
            (f_cfg, c_cfg, s_prompt)
        } else {
            (
                global_cfg.frontier_model.clone(),
                global_cfg.cheap_model.clone(),
                global_cfg.system_prompt(),
            )
        };

        let app_skill_rows: Vec<(String, Option<String>)> =
            sqlx::query_as::<_, (String, Option<String>)>(
                "SELECT name, description FROM app_skills ORDER BY name ASC",
            )
            .fetch_all(&self.pool)
            .await
            .unwrap_or_default();

        let user_skill_rows: Vec<(String, Option<String>)> =
            sqlx::query_as::<_, (String, Option<String>)>(
                "SELECT name, description FROM user_skills WHERE user_id = $1 ORDER BY name ASC",
            )
            .bind(user_id)
            .fetch_all(&self.pool)
            .await
            .unwrap_or_default();

        let mut skills = Vec::new();
        for (name, desc) in app_skill_rows.into_iter().chain(user_skill_rows) {
            if let Some(d) = desc {
                skills.push(format!("- {name}: {d}"));
            } else {
                skills.push(format!("- {name}"));
            }
        }

        if !skills.is_empty() {
            skills.sort();
            skills.dedup();
            let summary = format!(
                "\n\n可用 Skills（需要时调用 load_skill 获取详细指令）：\n{}",
                skills.join("\n")
            );
            system_prompt = Some(system_prompt.unwrap_or_default() + &summary);
        }

        let plan_row: Option<(String, String)> = sqlx::query_as(
            "SELECT title, steps_json FROM conversation_plans WHERE conversation_id = $1",
        )
        .bind(conversation_id)
        .fetch_optional(&self.pool)
        .await
        .unwrap_or(None);

        if let Some((plan_title, plan_steps)) = plan_row {
            let plan_section = format!(
                "\n\n## 当前执行计划\n标题：{}\n步骤（JSON）：{}\n\n每次更新步骤状态时，调用 todo_list 工具同步最新进度。",
                plan_title, plan_steps
            );
            system_prompt = Some(system_prompt.unwrap_or_default() + &plan_section);
        }

        let client = frontier_cfg.to_client();

        let ask_user_pending = Arc::new(tokio::sync::Mutex::new(None));
        let abort_flag = Arc::new(AtomicBool::new(false));
        let user_mcp_tools: Arc<tokio::sync::Mutex<Vec<(String, McpTool)>>> =
            Arc::new(tokio::sync::Mutex::new(Vec::new()));

        // OnceLock shared with ManageMcpSpell — filled in after the Arc is created below.
        let agent_once: Arc<tokio::sync::OnceCell<Arc<tokio::sync::Mutex<Agent>>>> =
            Arc::new(tokio::sync::OnceCell::new());

        let spell_deps = SpellDeps {
            ask_pending: Arc::clone(&ask_user_pending),
            subagent_prompt: global_cfg.subagent_prompt(),
            cheap_model: cheap_cfg,
            mcp_tools: Arc::clone(&user_mcp_tools),
            spawn_tx: spawn_tx.clone(),
            db: self.db.clone(),
            embed: EmbeddingClient::new(
                global_cfg.embedding.api_key.clone(),
                global_cfg.embedding.api_base.clone(),
                global_cfg.embedding.name.clone(),
            ),
            conversation_id,
            agent: Arc::clone(&agent_once),
            pool: self.pool.clone(),
            user_id,
            sandbox: self.sandbox.clone(),
            mcp_catalog: global_cfg.mcp_catalog.clone(),
            abort_flag: Arc::clone(&abort_flag),
        };

        // Build agent once, cleanly — no double construction.
        let mut agent = Agent::new(client);
        if let Some(prompt) = system_prompt {
            let rendered = shared_backend::prompt_template::render_prompt(
                &prompt,
                &[("USER_NAME", &user_name)],
            );
            agent = agent.system_prompt(rendered);
        }
        agent = agent.tool(build_all_spells(spell_deps));
        for (_, tool) in &mcp_snapshot {
            agent = agent.tool(tool.clone());
        }
        agent = agent.memory(InMemory::new().with_history(history));

        let agent_arc = Arc::new(tokio::sync::Mutex::new(agent));
        // Set the OnceLock so ManageMcpSpell can use the agent for hot-swapping.
        let _ = agent_once.set(Arc::clone(&agent_arc));

        // Now connect user MCPs
        for (name, mcp_type, config) in user_mcp_rows {
            let tool = match mcp_type.as_str() {
                "http" => {
                    let url = config
                        .get("url")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string();
                    match tokio::time::timeout(Duration::from_secs(15), McpTool::http(&url)).await {
                        Ok(Ok(t)) => t,
                        Ok(Err(e)) => {
                            warn!("user MCP '{name}' failed to connect: {e}");
                            continue;
                        }
                        Err(_) => {
                            warn!("user MCP '{name}' connection timed out (15s), skipping");
                            continue;
                        }
                    }
                }
                "stdio" => {
                    let command = config
                        .get("command")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string();
                    let args: Vec<String> = config
                        .get("args")
                        .and_then(|v| v.as_array())
                        .map(|a| {
                            a.iter()
                                .filter_map(|v| v.as_str().map(str::to_string))
                                .collect()
                        })
                        .unwrap_or_default();
                    let args_ref: Vec<&str> = args.iter().map(String::as_str).collect();
                    let (cmd, args_wrapped) = self.sandbox.wrap_mcp_command(user_id, conversation_id, &command, &args_ref);
                    let args_wrapped_ref: Vec<&str> = args_wrapped.iter().map(String::as_str).collect();
                    match tokio::time::timeout(
                        Duration::from_secs(300),
                        McpTool::stdio(&cmd, &args_wrapped_ref),
                    )
                    .await
                    {
                        Ok(Ok(t)) => t,
                        Ok(Err(e)) => {
                            warn!("user MCP '{name}' failed to start: {e}");
                            continue;
                        }
                        Err(_) => {
                            warn!("user MCP '{name}' startup timed out (300s), skipping");
                            continue;
                        }
                    }
                }
                _ => continue,
            };

            info!("user MCP '{name}' ready ({} tools)", tool.raw_tools().len());
            user_mcp_tools.lock().await.push((name.clone(), tool.clone()));
            let mut agent = agent_arc.lock().await;
            agent.add_tool(tool).await;
        }

        // If the user's desktop client tunnel is online, add those tools too
        {
            let registry = self.tunnel_registry.lock().await;
            if let Some(tunnel_tool) = registry.get(&user_id) {
                info!(
                    "用户 {user_id} 隧道工具已在线，加入 agent ({} tools)",
                    tunnel_tool.raw_tools().len()
                );
                let mut agent = agent_arc.lock().await;
                agent.add_tool(tunnel_tool.clone()).await;
            }
        }

        // Relay spawn_tx messages to the chat entry emitter
        let relay_state = self.clone();
        let mut rx = spawn_tx.subscribe();
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(payload) => {
                        let mut map = relay_state.chats.lock().unwrap();
                        if let Some(e) = map.get_mut(&conversation_id) {
                            e.emit(payload);
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        });

        agent_arc
    }

    pub async fn deliver_answer(&self, conversation_id: Uuid, answer: String) {
        let pending = {
            let map = self.chats.lock().unwrap();
            map.get(&conversation_id)
                .map(|e| Arc::clone(&e.ask_user_pending))
        };

        if let Some(pending) = pending {
            let mut guard = pending.lock().await;
            if let Some(tx) = guard.take() {
                let _ = tx.send(answer);
            }
        }
    }

    pub async fn attach(
        &self,
        conversation_id: Uuid,
    ) -> (broadcast::Receiver<Arc<WsEvent>>, Vec<Arc<WsEvent>>, bool) {
        {
            let map = self.chats.lock().unwrap();
            if let Some(entry) = map.get(&conversation_id) {
                let rx = entry.broadcast_tx.subscribe();
                let log: Vec<Arc<WsEvent>> = entry.event_log.iter().cloned().collect();
                let generating = entry.generating;
                return (rx, log, generating);
            }
        }

        let user_id: Uuid =
            sqlx::query_scalar::<_, Uuid>("SELECT user_id FROM conversations WHERE id = $1")
                .bind(conversation_id)
                .fetch_optional(&self.pool)
                .await
                .unwrap_or(None)
                .unwrap_or_else(|| {
                    warn!(conversation = %conversation_id, "conversation not found in attach, using nil user_id");
                    Uuid::nil()
                });

        let (spawn_tx, _) = tokio::sync::broadcast::channel::<String>(256);
        let ask_user_pending = Arc::new(tokio::sync::Mutex::new(None));
        let abort_flag = Arc::new(AtomicBool::new(false));
        let user_mcp_tools: Arc<tokio::sync::Mutex<Vec<(String, McpTool)>>> =
            Arc::new(tokio::sync::Mutex::new(Vec::new()));

        let agent_arc = self.build_agent(conversation_id).await;

        let entry = ChatEntry::new(
            user_id,
            agent_arc,
            spawn_tx,
            user_mcp_tools,
            ask_user_pending,
            abort_flag,
        );

        let rx = entry.broadcast_tx.subscribe();
        let log: Vec<Arc<WsEvent>> = entry.event_log.iter().cloned().collect();
        let generating = entry.generating;
        self.chats.lock().unwrap().insert(conversation_id, entry);

        (rx, log, generating)
    }

    pub async fn start_generation(&self, conversation_id: Uuid, user_parts: Vec<agentix::UserContent>) -> bool {
        let (agent_arc, abort_flag) = {
            let mut map = self.chats.lock().unwrap();
            let entry = match map.get_mut(&conversation_id) {
                Some(e) => e,
                None => return false,
            };

            if entry.generating {
                return false;
            }

            entry.clear_log();
            entry.abort_flag.store(false, Ordering::Release);
            entry.generating = true;
            (Arc::clone(&entry.agent), Arc::clone(&entry.abort_flag))
        };

        let state = self.clone();
        tokio::spawn(async move {
            generation_loop(state, conversation_id, agent_arc, abort_flag, user_parts).await;
        });

        true
    }

    pub fn send_interrupt(&self, conversation_id: Uuid, content: String) {
        let mut map = self.chats.lock().unwrap();
        if let Some(entry) = map.get_mut(&conversation_id) {
            entry.queued_interrupts.push(content);
        }
    }

    pub fn abort_generation(&self, conversation_id: Uuid) {
        let map = self.chats.lock().unwrap();
        if let Some(entry) = map.get(&conversation_id) {
            entry.abort_flag.store(true, Ordering::Release);
        }
    }

    pub fn persist_message(&self, conversation_id: Uuid, msg: &Message) {
        let state = self.clone();
        let msg = msg.clone();
        tokio::spawn(async move {
            state.persist_message_async(conversation_id, msg).await;
        });
    }

    pub async fn persist_message_async(&self, conversation_id: Uuid, msg: Message) {
        let db = self.db.clone();
        let state = self.clone();

        let text_for_embed: Option<String> = match &msg {
            Message::User(parts) => {
                use agentix::UserContent;
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
                error!("db append failed: {e}");
                return;
            }
        };

        if let Some(content) = text_for_embed {
            tokio::spawn(async move {
                let global_cfg = state.get_global_config().await.unwrap_or_default();
                let embed = EmbeddingClient::new(
                    global_cfg.embedding.api_key,
                    global_cfg.embedding.api_base,
                    global_cfg.embedding.name,
                );
                match embed.embed(&content).await {
                    Ok(vec) => {
                        let vector = to_vector(vec);
                        if let Err(e) = db.set_embedding(row_id, vector).await {
                            error!("set_embedding failed: {e}");
                        }
                    }
                    Err(e) => error!("embed failed: {e}"),
                }
            });
        }
    }
}

async fn generation_loop(
    state: AppState,
    conversation_id: Uuid,
    agent_arc: Arc<tokio::sync::Mutex<Agent>>,
    abort_flag: Arc<AtomicBool>,
    user_parts: Vec<agentix::UserContent>,
) {
    let mut first = Some(user_parts);
    let mut interrupt_text: Option<String> = None;
    loop {
        let parts = if let Some(p) = first.take() {
            p
        } else if let Some(t) = interrupt_text.take() {
            vec![agentix::UserContent::Text(t)]
        } else {
            break;
        };
        let next = run_generation(
            state.clone(),
            conversation_id,
            Arc::clone(&agent_arc),
            Arc::clone(&abort_flag),
            parts,
        )
        .await;
        match next {
            Some(interrupt) => interrupt_text = Some(interrupt),
            None => break,
        }
    }
}

/// Returns `Some(interrupt_text)` if a queued interrupt should trigger another turn,
/// or `None` to stop the loop.
async fn run_generation(
    state: AppState,
    conversation_id: Uuid,
    agent_arc: Arc<tokio::sync::Mutex<Agent>>,
    abort_flag: Arc<AtomicBool>,
    user_parts: Vec<agentix::UserContent>,
) -> Option<String> {
    debug!(conversation = %conversation_id, "[TIMING] run_generation started");

    let mut stream = {
        let mut agent = agent_arc.lock().await;
        match agent.chat_multimodal(user_parts).await {
            Ok(s) => s,
            Err(e) => {
                error!(conversation = %conversation_id, "chat() failed: {e}");
                let mut map = state.chats.lock().unwrap();
                if let Some(entry) = map.get_mut(&conversation_id) {
                    entry.emit(json!({"type": "error", "message": e.to_string()}).to_string());
                    entry.generating = false;
                }
                return None;
            }
        }
    };

    let mut reply_buf = String::new();
    let mut reasoning_buf = String::new();
    let mut tool_calls_buf: Vec<agentix::ToolCall> = Vec::new();
    let mut poll_count = 0u32;
    let mut usage = UsageStats::default();

    loop {
        if abort_flag.load(Ordering::Acquire) {
            {
                let mut agent = agent_arc.lock().await;
                let _ = agent.abort().await;
            }
            if !reply_buf.is_empty() {
                state.persist_message(
                    conversation_id,
                    &Message::Assistant {
                        content: Some(reply_buf.clone()),
                        reasoning: None,
                        tool_calls: vec![],
                    },
                );
            }
            let mut map = state.chats.lock().unwrap();
            if let Some(entry) = map.get_mut(&conversation_id) {
                entry.emit(json!({"type": "aborted"}).to_string());
                entry.generating = false;
                entry.abort_flag.store(false, Ordering::Release);
            }
            return None;
        }

        poll_count += 1;
        debug!(conversation = %conversation_id, "[TIMING] poll #{poll_count}: calling stream.next()");

        let event = stream.next().await;
        match event {
            None => break,
            Some(AgentEvent::Done) => break,
            Some(AgentEvent::Token(token)) => {
                reply_buf.push_str(&token);
                let payload = json!({"type": "token", "content": token}).to_string();
                let mut map = state.chats.lock().unwrap();
                if let Some(entry) = map.get_mut(&conversation_id) {
                    entry.emit(payload);
                }
            }
            Some(AgentEvent::Reasoning(token)) => {
                reasoning_buf.push_str(&token);
                let payload = json!({"type": "reasoning_token", "content": token}).to_string();
                let mut map = state.chats.lock().unwrap();
                if let Some(entry) = map.get_mut(&conversation_id) {
                    entry.emit(payload);
                }
            }
            Some(AgentEvent::ToolCallChunk(c)) => {
                let payload = json!({"type": "tool_call", "id": c.id, "name": c.name, "delta": c.delta}).to_string();
                let mut map = state.chats.lock().unwrap();
                if let Some(entry) = map.get_mut(&conversation_id) {
                    entry.emit(payload);
                }
            }
            Some(AgentEvent::ToolCall(c)) => {
                let payload = json!({"type": "tool_call_complete", "id": c.id, "name": c.name}).to_string();
                tool_calls_buf.push(c);
                let mut map = state.chats.lock().unwrap();
                if let Some(entry) = map.get_mut(&conversation_id) {
                    entry.emit(payload);
                }
            }
            Some(AgentEvent::ToolProgress { call_id, name, progress }) => {
                let payload = json!({"type": "tool_progress", "id": call_id, "name": name, "progress": progress}).to_string();
                let mut map = state.chats.lock().unwrap();
                if let Some(entry) = map.get_mut(&conversation_id) {
                    entry.emit(payload);
                }
            }
            Some(AgentEvent::ToolResult { call_id, name, result }) => {
                {
                    let mut map = state.chats.lock().unwrap();
                    if let Some(entry) = map.get_mut(&conversation_id) {
                        entry.queued_interrupts.clear();
                    }
                }
                let payload = json!({"type": "tool_result", "id": call_id, "name": name, "result": result}).to_string();
                let mut map = state.chats.lock().unwrap();
                if let Some(entry) = map.get_mut(&conversation_id) {
                    entry.emit(payload);
                }
            }
            Some(AgentEvent::Error(err_msg)) => {
                error!(conversation = %conversation_id, "stream error: {err_msg}");
                let is_benign = err_msg.contains("Error in input stream") && !reply_buf.trim().is_empty();
                if is_benign {
                    warn!(conversation = %conversation_id, "treating benign tail error as done");
                    break;
                }
                let payload = json!({"type": "error", "message": err_msg}).to_string();
                let mut map = state.chats.lock().unwrap();
                if let Some(entry) = map.get_mut(&conversation_id) {
                    entry.emit(payload);
                    entry.generating = false;
                }
                return None;
            }
            Some(AgentEvent::Usage(u)) => {
                usage += u;
            }
            Some(_) => {}
        }
    }

    if usage.total_tokens > 0 {
        let pool = state.pool.clone();
        tokio::spawn(async move {
            let _ = sqlx::query(
                r#"UPDATE conversations
                   SET token_usage = token_usage || jsonb_build_object(
                       'prompt_tokens',     (COALESCE((token_usage->>'prompt_tokens')::bigint, 0) + $1),
                       'completion_tokens', (COALESCE((token_usage->>'completion_tokens')::bigint, 0) + $2),
                       'total_tokens',      (COALESCE((token_usage->>'total_tokens')::bigint, 0) + $3)
                   )
                   WHERE id = $4"#,
            )
            .bind(usage.prompt_tokens as i64)
            .bind(usage.completion_tokens as i64)
            .bind(usage.total_tokens as i64)
            .bind(conversation_id)
            .execute(&pool)
            .await;
        });

        let payload = json!({
            "type": "usage",
            "prompt_tokens": usage.prompt_tokens,
            "completion_tokens": usage.completion_tokens,
            "total_tokens": usage.total_tokens,
        }).to_string();
        let mut map = state.chats.lock().unwrap();
        if let Some(entry) = map.get_mut(&conversation_id) {
            entry.emit(payload);
        }
    }

    if !reply_buf.is_empty() || !tool_calls_buf.is_empty() {
        state.persist_message(
            conversation_id,
            &Message::Assistant {
                content: if reply_buf.is_empty() { None } else { Some(reply_buf) },
                reasoning: if reasoning_buf.is_empty() { None } else { Some(reasoning_buf) },
                tool_calls: tool_calls_buf,
            },
        );
    }

    

    {
        let mut map = state.chats.lock().unwrap();
        if let Some(entry) = map.get_mut(&conversation_id) {
            let next_interrupt = entry.queued_interrupts.drain(..).next();
            if next_interrupt.is_some() {
                entry.emit(json!({"type": "user_interrupt"}).to_string());
            } else {
                entry.emit(json!({"type": "done"}).to_string());
                entry.generating = false;
            }
            next_interrupt
        } else {
            None
        }
    }
}
