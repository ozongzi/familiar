use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::config::{Config, McpServerConfig, ModelConfig};
use ds_api::AgentEvent;
use ds_api::DeepseekAgent;
use ds_api::McpTool;
use ds_api::Tool as _;
use ds_api::ToolInjection;
use serde_json::Value;
use sqlx::PgPool;
use tokio::sync::broadcast;
use tokio::sync::mpsc::UnboundedSender;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::db::{Db, to_vector};
use crate::embedding::EmbeddingClient;
use crate::spells::{SpellDeps, build_all_spells};
use std::sync::atomic::{AtomicBool, Ordering};

// How many events to keep in the log for late-joining clients.
// Enough to replay a full long turn including many tool calls.
const EVENT_LOG_CAP: usize = 4096;

// broadcast channel capacity — how many events can be buffered before
// a slow subscriber starts missing messages.
const BROADCAST_CAP: usize = 256;

/// Global sequence counter — every emitted event gets a unique, monotonically
/// increasing sequence number so SSE clients can deduplicate replays exactly.
static NEXT_SEQ: AtomicU64 = AtomicU64::new(0);

/// A single event that was (or will be) sent over WebSocket.
/// Stored in the event log so late-joining clients can replay.
#[derive(Debug, Clone)]
pub struct WsEvent {
    /// Globally unique, monotonically increasing sequence number.
    /// The SSE client uses this to skip events it has already seen
    /// during reconnect replay, replacing the fragile Arc-pointer trick.
    pub seq: u64,
    pub payload: String, // serialised JSON, ready to send
}

/// One entry per conversation.
pub struct ChatEntry {
    /// Owner user id for this conversation (used to target per-user injections).
    pub user_id: Uuid,

    /// The agent when idle (not generating). Taken out during generation.
    pub agent: Option<DeepseekAgent>,

    /// Kept alive so the agent's interrupt receiver is never dropped.
    pub interrupt_tx: UnboundedSender<String>,

    /// Channel used to inject/remove tools into the running agent.
    pub tool_inject_tx: UnboundedSender<ToolInjection>,

    /// Per-session user MCP tools (user-level MCPs loaded for this agent instance).
    /// Stored so spells and management code can inspect or mutate session-scoped MCPs.
    pub user_mcp_tools: Arc<tokio::sync::Mutex<Vec<(String, McpTool)>>>,

    /// Broadcast sender — the background generation task sends every event here.
    /// WebSocket handlers subscribe to receive live events.
    pub broadcast_tx: broadcast::Sender<Arc<WsEvent>>,

    /// Ordered log of every event emitted in the current (or most recent) turn.
    /// New WebSocket clients replay this before subscribing to live events so
    /// they catch up even if they connected mid-generation or after it finished.
    pub event_log: Vec<Arc<WsEvent>>,

    /// True while a background generation task is running for this conversation.
    pub generating: bool,

    /// Set to true by ws.rs when the client sends { type: "abort" }.
    /// The generation task polls this flag and stops early when it's set.
    pub abort_flag: Arc<AtomicBool>,

    /// Shared slot for the ask_user spell: while the spell is awaiting user input it
    /// stores a oneshot::Sender here; ws.rs extracts and fires it with the user's reply.
    pub ask_user_pending: Arc<tokio::sync::Mutex<Option<tokio::sync::oneshot::Sender<String>>>>,

    /// Interrupt messages that haven't been consumed by the agent yet.
    /// Populated by send_interrupt alongside the agent's internal channel.
    /// Cleared after each ToolResult (the agent drains its channel then).
    /// Any messages remaining after "done" are processed as a new generation.
    pub queued_interrupts: Vec<String>,

    /// Broadcast sender for sub-agent (SpawnSpell) events.
    /// start_generation subscribes a fresh receiver each turn and forwards
    /// events into the main WS broadcast stream.
    pub spawn_tx: tokio::sync::broadcast::Sender<String>,
}

impl ChatEntry {
    fn new(
        user_id: Uuid,
        agent: DeepseekAgent,
        (interrupt_tx, tool_inject_tx, spawn_tx): (
            UnboundedSender<String>,
            UnboundedSender<ToolInjection>,
            tokio::sync::broadcast::Sender<String>,
        ),
        user_mcp_tools: Arc<tokio::sync::Mutex<Vec<(String, McpTool)>>>,
        ask_user_pending: Arc<tokio::sync::Mutex<Option<tokio::sync::oneshot::Sender<String>>>>,
        abort_flag: Arc<AtomicBool>,
    ) -> Self {
        let (broadcast_tx, _) = broadcast::channel(BROADCAST_CAP);
        Self {
            user_id,
            agent: Some(agent),
            interrupt_tx,
            tool_inject_tx,
            user_mcp_tools,
            broadcast_tx,
            event_log: Vec::new(),
            generating: false,
            abort_flag,
            ask_user_pending,
            queued_interrupts: Vec::new(),
            spawn_tx,
        }
    }

    /// Emit an event: append to the log and broadcast to all live subscribers.
    /// The log is capped at EVENT_LOG_CAP to avoid unbounded memory growth.
    pub fn emit(&mut self, payload: String) {
        let seq = NEXT_SEQ.fetch_add(1, AtomicOrdering::Relaxed);
        let ev = Arc::new(WsEvent { seq, payload });
        if self.event_log.len() >= EVENT_LOG_CAP {
            self.event_log.remove(0);
        }
        self.event_log.push(Arc::clone(&ev));
        // Ignore send errors — no subscribers is fine.
        let _ = self.broadcast_tx.send(ev);
    }

    /// Clear the event log for a new generation turn.
    pub fn clear_log(&mut self) {
        self.event_log.clear();
    }
}

/// Shared application state, held behind `Arc`.
#[allow(unused)]
#[derive(Clone)]
pub struct AppState {
    /// Per-conversation agent instances, keyed by conversation UUID.
    pub chats: Arc<Mutex<HashMap<Uuid, ChatEntry>>>,

    /// SSE stream tokens: stream_id → (conversation_id, user_id).
    /// Created by `create_stream`, consumed by `resolve_stream`.
    pub streams: Arc<Mutex<HashMap<Uuid, (Uuid, Uuid)>>>,

    pub frontier_model: ModelConfig,

    pub cheap_model: ModelConfig,

    /// Optional system prompt applied to every freshly created agent.
    pub system_prompt: Option<String>,

    pub subagent_prompt: Option<String>,

    /// PostgreSQL connection pool.
    pub pool: PgPool,

    /// Thin wrapper around the pool for message persistence.
    pub db: Db,

    /// Embedding client — shared across all conversations.
    pub embed: EmbeddingClient,

    /// Sandbox manager — handles per-user Docker containers.
    pub sandbox: Arc<crate::sandbox::SandboxManager>,

    /// Named MCP tools — started at startup and updated dynamically by
    /// ManageMcpSpell. Wrapped in Arc<Mutex> so the spell can add/remove
    /// entries without going through AppState.
    pub mcp_tools: Arc<tokio::sync::Mutex<Vec<(String, McpTool)>>>,
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
            frontier_model: cfg.frontier_model.clone(),
            cheap_model: cfg.cheap_model.clone(),
            system_prompt: cfg.system_prompt(),
            subagent_prompt: cfg.subagent_prompt(),
            pool,
            db,
            embed: EmbeddingClient::new(
                cfg.embedding.api_key.clone(),
                cfg.embedding.api_base.clone(),
                cfg.embedding.name.clone(),
            ),
            sandbox,
            mcp_tools: Arc::new(tokio::sync::Mutex::new(mcp_tools)),
        }
    }

    /// Create a new SSE stream token for the given conversation and user.
    /// Returns the generated stream_id (a random UUID).
    pub fn create_stream(&self, conversation_id: Uuid, user_id: Uuid) -> Uuid {
        let stream_id = Uuid::new_v4();
        self.streams
            .lock()
            .unwrap()
            .insert(stream_id, (conversation_id, user_id));
        stream_id
    }

    /// Resolve a stream_id to its (conversation_id, user_id) pair.
    /// Returns None if the token has expired or never existed.
    pub fn resolve_stream(&self, stream_id: Uuid) -> Option<(Uuid, Uuid)> {
        self.streams.lock().unwrap().get(&stream_id).copied()
    }

    /// Initialise MCP servers from config. Called once at startup.
    /// Failures are logged and skipped — a missing MCP server should never
    /// prevent familiar from starting.
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
                    // When env vars are specified, prepend them via `env KEY=VAL ... cmd args`
                    // so they are injected into the subprocess without touching the parent process.
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

    /// Build a fresh agent for `conversation_id`, restoring history from PG.
    /// Returns `(agent, interrupt_tx, ask_user_pending, tool_inject_tx, spawn_tx, abort_flag)`.
    pub async fn build_agent(
        &self,
        conversation_id: Uuid,
    ) -> (
        DeepseekAgent,
        UnboundedSender<String>,
        Arc<tokio::sync::Mutex<Option<tokio::sync::oneshot::Sender<String>>>>,
        tokio::sync::mpsc::UnboundedSender<ToolInjection>,
        Arc<tokio::sync::Mutex<Vec<(String, McpTool)>>>, // per-agent user mcp tools
        tokio::sync::broadcast::Sender<String>,
        Arc<AtomicBool>, // abort_flag
    ) {
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

        let ask_user_pending = Arc::new(tokio::sync::Mutex::new(None));
        let abort_flag = Arc::new(AtomicBool::new(false));

        // Snapshot the global (system-level) MCP tools.
        let mcp_snapshot: Vec<_> = {
            let guard = self.mcp_tools.lock().await;
            guard.iter().cloned().collect()
        };

        // Resolve user_id from conversation, then load per-user MCPs from DB.
        let user_id: Uuid =
            sqlx::query_scalar::<_, Uuid>("SELECT user_id FROM conversations WHERE id = $1")
                .bind(conversation_id)
                .fetch_optional(&self.pool)
                .await
                .unwrap_or(None)
                .unwrap_or_default();

        let user_mcp_rows: Vec<(String, String, serde_json::Value)> = sqlx::query_as(
            r#"SELECT name, "type", config FROM user_mcps WHERE user_id = $1 ORDER BY created_at ASC"#
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default();

        let (spawn_tx, _) = tokio::sync::broadcast::channel::<String>(256);

        // Fetch user settings (models, system prompt) from DB.
        let user_settings: Option<(Option<Value>, Option<Value>, Option<String>)> = sqlx::query_as(
            "SELECT frontier_model, cheap_model, system_prompt FROM user_settings WHERE user_id = $1"
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
        .unwrap_or(None);

        // Determine base frontier/cheap configs and the initial system prompt.
        let (frontier_cfg, cheap_cfg, mut system_prompt) = if let Some((f, c, p)) = user_settings {
            let f_cfg: ModelConfig = f.and_then(|v| serde_json::from_value(v).ok())
                .unwrap_or_else(|| self.frontier_model.clone());
            let c_cfg: ModelConfig = c.and_then(|v| serde_json::from_value(v).ok())
                .unwrap_or_else(|| self.cheap_model.clone());
            let s_prompt = p.or_else(|| self.system_prompt.clone());
            (f_cfg, c_cfg, s_prompt)
        } else {
            (self.frontier_model.clone(), self.cheap_model.clone(), self.system_prompt.clone())
        };

        // Append per-user skills summary (from DB) to the system prompt if any skills exist.
        // This mirrors Config::skills_summary but reads from `user_skills` for the current user.
        let skill_rows: Vec<(String, Option<String>)> = sqlx::query_as::<_, (String, Option<String>)>(
            "SELECT name, description FROM user_skills WHERE user_id = $1 ORDER BY name ASC"
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default();

        if !skill_rows.is_empty() {
            let mut skills = Vec::new();
            for (name, desc) in skill_rows {
                if let Some(d) = desc {
                    skills.push(format!("- {name}: {d}"));
                } else {
                    skills.push(format!("- {name}"));
                }
            }
            // Ensure deterministic order
            skills.sort();
            let summary = format!(
                "\n\n可用 Skills（需要时调用 load_skill 获取详细指令）：\n{}",
                skills.join("\n")
            );
            // Append summary to whatever system_prompt we have (or start with it).
            system_prompt = Some(system_prompt.unwrap_or_default() + &summary);
        }

        // Build the base agent (without spells yet) and attach both channels.
        // We need inject_tx before building SpellDeps, so attach the tool-inject
        // channel first, then clone the tx for spells.
        let mut base = DeepseekAgent::custom(
            frontier_cfg.api_key.clone(),
            frontier_cfg.api_base.clone(),
            frontier_cfg.name.clone(),
        )
        .with_streaming()
        .with_history(history);

        for (k, v) in &frontier_cfg.extra_body {
            base = base.extra_field(k.clone(), v.clone());
        }

        if let Some(prompt) = &system_prompt {
            base = base.with_system_prompt(prompt.clone());
        }

        let interrupt_tx = base.interrupt_sender();
        let inject_tx = base.tool_inject_sender();

        // Per-user mcp_tools Arc (scoped to this agent instance).
        let user_mcp_tools: Arc<tokio::sync::Mutex<Vec<(String, McpTool)>>> =
            Arc::new(tokio::sync::Mutex::new(Vec::new()));

        let spell_deps = SpellDeps {
            subagent_prompt: self.subagent_prompt.clone(),
            ask_pending: Arc::clone(&ask_user_pending),
            cheap_model: cheap_cfg,
            mcp_tools: Arc::clone(&user_mcp_tools),
            spawn_tx: spawn_tx.clone(),
            db: self.db.clone(),
            embed: self.embed.clone(),
            conversation_id,
            tool_inject_tx: inject_tx.clone(),
            pool: self.pool.clone(),
            user_id,
            sandbox: self.sandbox.clone(),
            abort_flag: Arc::clone(&abort_flag),
        };

        let mut agent = base.add_tool(build_all_spells(spell_deps));

        // Add system-level MCP tools (from config).
        for (_, tool) in mcp_snapshot {
            agent = agent.add_tool(tool);
        }

        // Connect per-user MCPs from DB and add to agent.
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

                    // Wrap the stdio command with Docker exec for sandboxing
                    let (cmd, args) = self.sandbox.wrap_mcp_command(user_id, &command, &args_ref);
                    let args_wrapped: Vec<&str> = args.iter().map(String::as_str).collect();

                    match tokio::time::timeout(
                        Duration::from_secs(300), // Increased timeout for docker
                        McpTool::stdio(&cmd, &args_wrapped),
                    )
                    .await
                    {
                        Ok(Ok(t)) => t,
                        Ok(Err(e)) => {
                            warn!("user MCP '{name}' failed to start: {e}");
                            continue;
                        }
                        Err(_) => {
                            warn!("user MCP '{name}' startup timed out (30s), skipping");
                            continue;
                        }
                    }
                }
                _ => continue,
            };
            info!("user MCP '{name}' ready ({} tools)", tool.raw_tools().len());
            user_mcp_tools
                .lock()
                .await
                .push((name.clone(), tool.clone()));
            agent = agent.add_tool(tool);
        }

        (
            agent,
            interrupt_tx,
            ask_user_pending,
            inject_tx,
            user_mcp_tools,
            spawn_tx,
            abort_flag,
        )
    }

    /// Deliver a user's answer to a waiting `ask_user` spell.
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

    /// Ensure a `ChatEntry` exists for `conversation_id`, building one if needed.
    /// Returns a broadcast receiver (for live events) and the full event log
    /// snapshot (for replay), plus whether generation is currently in progress.
    pub async fn attach(
        &self,
        conversation_id: Uuid,
    ) -> (broadcast::Receiver<Arc<WsEvent>>, Vec<Arc<WsEvent>>, bool) {
        // Fast path — entry already exists.
        {
            let map = self.chats.lock().unwrap();
            if let Some(entry) = map.get(&conversation_id) {
                let rx = entry.broadcast_tx.subscribe();
                let log = entry.event_log.clone();
                let generating = entry.generating;
                return (rx, log, generating);
            }
        }

        // Slow path — build agent outside the lock.
        // Defer building the agent until we resolve the conversation's user_id below.

        // Resolve owner user_id for this conversation so we can tag the session entry.
        let user_id: Uuid =
            sqlx::query_scalar::<_, Uuid>("SELECT user_id FROM conversations WHERE id = $1")
                .bind(conversation_id)
                .fetch_optional(&self.pool)
                .await
                .unwrap_or(None)
                .unwrap_or_default();

        let (agent, tx, ask_user_pending, inject_tx, user_mcp_tools, spawn_tx, abort_flag) =
            self.build_agent(conversation_id).await;
        let entry = ChatEntry::new(
            user_id,
            agent,
            (tx, inject_tx, spawn_tx),
            user_mcp_tools,
            ask_user_pending,
            abort_flag,
        );
        let rx = entry.broadcast_tx.subscribe();
        let log = entry.event_log.clone();
        let generating = entry.generating;
        self.chats.lock().unwrap().insert(conversation_id, entry);
        (rx, log, generating)
    }

    /// Start a background generation task for `conversation_id`.
    ///
    /// Pushes `user_text` onto the agent, marks the entry as `generating`,
    /// clears the event log, and spawns a task that drives the agent stream,
    /// emitting every event through the broadcast channel and the log.
    ///
    /// Returns `false` if generation is already in progress (caller should
    /// send the event log replay + subscribe instead of starting a new turn).
    pub async fn start_generation(&self, conversation_id: Uuid, user_text: String) -> bool {
        // Take the agent out of the entry (if idle).
        let (agent, abort_flag) = {
            let mut map = self.chats.lock().unwrap();
            let entry = match map.get_mut(&conversation_id) {
                Some(e) => e,
                None => return false,
            };
            if entry.generating {
                return false;
            }
            // Clear previous turn's log and reset abort flag.
            entry.clear_log();
            entry.abort_flag.store(false, Ordering::Release);
            entry.generating = true;
            (entry.agent.take(), Arc::clone(&entry.abort_flag))
        };

        let mut agent = match agent {
            Some(a) => a,
            None => {
                // Agent missing (shouldn't happen but be defensive) — rebuild.
                let (
                    fresh_agent,
                    fresh_tx,
                    fresh_pending,
                    fresh_inject_tx,
                    fresh_user_mcp_tools,
                    fresh_spawn_tx,
                    fresh_abort,
                ) = self.build_agent(conversation_id).await;
                let mut map = self.chats.lock().unwrap();
                if let Some(entry) = map.get_mut(&conversation_id) {
                    entry.interrupt_tx = fresh_tx;
                    entry.tool_inject_tx = fresh_inject_tx;
                    entry.user_mcp_tools = fresh_user_mcp_tools;
                    entry.ask_user_pending = fresh_pending;
                    entry.spawn_tx = fresh_spawn_tx;
                    entry.abort_flag = Arc::clone(&fresh_abort);
                }
                fresh_agent
            }
        };

        agent.push_user_message_with_name(&user_text, None);

        let state = self.clone();

        // 把子 Agent（SpawnSpell）的流式事件转发到主 WS 广播流。
        // 每次 generation 开始时订阅一个新 receiver，转发任务在 sender drop 后自动退出。
        {
            let relay_state = state.clone();
            let mut rx = {
                let map = relay_state.chats.lock().unwrap();
                map.get(&conversation_id).map(|e| e.spawn_tx.subscribe())
            };
            if let Some(mut rx) = rx.take() {
                tokio::spawn(async move {
                    loop {
                        match rx.recv().await {
                            Ok(payload) => {
                                let mut map = relay_state.chats.lock().unwrap();
                                if let Some(entry) = map.get_mut(&conversation_id) {
                                    entry.emit(payload);
                                }
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                                continue;
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                                break;
                            }
                        }
                    }
                });
            }
        }

        tokio::spawn(async move {
            generation_loop(state, conversation_id, agent, abort_flag).await;
        });

        true
    }

    /// Inject a message into a running generation via the interrupt channel
    /// and queue it for post-generation processing in case the agent is on
    /// its final turn (no pending tool calls to trigger channel draining).
    pub fn send_interrupt(&self, conversation_id: Uuid, content: String) {
        let mut map = self.chats.lock().unwrap();
        if let Some(entry) = map.get_mut(&conversation_id) {
            let _ = entry.interrupt_tx.send(content.clone());
            entry.queued_interrupts.push(content);
        }
    }

    /// Signal the running generation task to stop as soon as possible.
    /// The task will emit an { type: "aborted" } event, persist what it
    /// has so far, recover the agent, and return.
    pub fn abort_generation(&self, conversation_id: Uuid) {
        let map = self.chats.lock().unwrap();
        if let Some(entry) = map.get(&conversation_id) {
            entry.abort_flag.store(true, Ordering::Release);
        }
    }

    /// Append a message to the database, awaiting the INSERT before returning.
    /// This guarantees the BIGSERIAL id is allocated in call order, so that
    /// assistant messages always get a lower id than the tool messages that follow.
    /// Embedding is still kicked off asynchronously so it doesn't block the caller.
    pub async fn persist_message_async(
        &self,
        conversation_id: Uuid,
        msg: &ds_api::raw::request::message::Message,
    ) {
        let db = self.db.clone();
        let embed = self.embed.clone();
        let msg = msg.clone();

        let row_id = match db.append(conversation_id, &msg, None).await {
            Ok(id) => id,
            Err(e) => {
                error!("db append failed: {e}");
                return;
            }
        };

        let should_embed = matches!(
            msg.role,
            ds_api::raw::request::message::Role::User
                | ds_api::raw::request::message::Role::Assistant
        );

        if should_embed
            && let Some(text) = &msg.content
            && !text.is_empty()
        {
            let text = text.clone();
            tokio::spawn(async move {
                match embed.embed(&text).await {
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

    /// Append a message to the database and kick off embedding in the background.
    /// Fire-and-forget: errors are logged, never propagated.
    pub fn persist_message(
        &self,
        conversation_id: Uuid,
        msg: &ds_api::raw::request::message::Message,
    ) {
        let db = self.db.clone();
        let embed = self.embed.clone();
        let msg = msg.clone();

        tokio::spawn(async move {
            let row_id = match db.append(conversation_id, &msg, None).await {
                Ok(id) => id,
                Err(e) => {
                    error!("db append failed: {e}");
                    return;
                }
            };

            let should_embed = matches!(
                msg.role,
                ds_api::raw::request::message::Role::User
                    | ds_api::raw::request::message::Role::Assistant
            );

            if should_embed
                && let Some(text) = &msg.content
                && !text.is_empty()
            {
                match embed.embed(text).await {
                    Ok(vec) => {
                        let vector = to_vector(vec);
                        if let Err(e) = db.set_embedding(row_id, vector).await {
                            error!("set_embedding failed: {e}");
                        }
                    }
                    Err(e) => error!("embed failed: {e}"),
                }
            }
        });
    }
}

// ── Background generation task ────────────────────────────────────────────────

/// Outer loop that runs `run_generation` and then restarts for any interrupt
/// that arrived during the agent's final turn (no tool calls to drain it).
/// By not calling `start_generation` recursively, we avoid a circular Send
/// constraint between `start_generation` and `run_generation`.
async fn generation_loop(
    state: AppState,
    conversation_id: Uuid,
    initial_agent: DeepseekAgent,
    initial_abort: Arc<AtomicBool>,
) {
    let mut agent = initial_agent;
    let mut abort_flag = initial_abort;

    loop {
        let pending_text =
            run_generation(state.clone(), conversation_id, agent, abort_flag.clone()).await;

        let text = match pending_text {
            None => break,
            Some(t) => t,
        };

        // A message arrived while the agent was on its last turn (no tool
        // calls, so the interrupt channel was never drained). Prepare the
        // entry for a new turn and loop back into run_generation.
        let next = {
            let mut map = state.chats.lock().unwrap();
            if let Some(entry) = map.get_mut(&conversation_id) {
                entry.clear_log();
                entry.abort_flag.store(false, Ordering::Release);
                entry.generating = true;
                let abort = Arc::clone(&entry.abort_flag);
                Some((entry.agent.take(), abort))
            } else {
                None
            }
        };

        let (agent_opt, new_abort) = match next {
            Some(t) => t,
            None => break,
        };

        let next_agent = match agent_opt {
            Some(a) => a,
            None => {
                // Agent missing (defensive) — rebuild from history.
                let (a, tx, pend, new_inject_tx, new_user_mcp_tools, new_spawn_tx, new_abort) =
                    state.build_agent(conversation_id).await;
                {
                    let mut map = state.chats.lock().unwrap();
                    if let Some(entry) = map.get_mut(&conversation_id) {
                        entry.interrupt_tx = tx;
                        entry.tool_inject_tx = new_inject_tx;
                        entry.user_mcp_tools = new_user_mcp_tools;
                        entry.ask_user_pending = pend;
                        entry.spawn_tx = new_spawn_tx;
                        entry.abort_flag = Arc::clone(&new_abort);
                    }
                }
                a
            }
        };

        let mut next_agent = next_agent;
        next_agent.push_user_message_with_name(&text, None);

        agent = next_agent;
        abort_flag = new_abort;
    }
}

/// Drives the agent stream to completion, emitting every event through the
/// broadcast channel and the event log so any number of WebSocket clients
/// can subscribe (or catch up after reconnecting).
///
/// Returns `Some(text)` if a user interrupt arrived during the final turn and
/// couldn't be processed inline; the caller (`generation_loop`) starts a new
/// turn for it.
async fn run_generation(
    state: AppState,
    conversation_id: Uuid,
    agent: DeepseekAgent,
    abort_flag: Arc<AtomicBool>,
) -> Option<String> {
    use ds_api::raw::request::message::{Message as AgentMessage, Role};
    use futures::StreamExt;
    use serde_json::json;

    info!(conversation = %conversation_id, "[TIMING] run_generation started, calling chat_from_history");
    let t_start = std::time::Instant::now();
    let mut stream = agent.chat_from_history();
    info!(conversation = %conversation_id, "[TIMING] chat_from_history returned in {:?}", t_start.elapsed());
    let mut reply_buf = String::new();
    let mut reasoning_buf = String::new();
    let mut poll_count = 0u32;
    // Accumulate tool calls for the current turn.
    // key = tool call id, value = (name, args, result_json)
    // result_json is None until the ToolResult arrives.
    let mut pending_tools: std::collections::HashMap<String, (String, String, Option<String>)> =
        std::collections::HashMap::new();
    // Preserve insertion order so the assistant message has tool_calls in the right order.
    let mut pending_tool_order: Vec<String> = Vec::new();
    // index -> name, for tool calls whose id hasn't arrived yet (MiniMax: empty id first)
    let mut pending_name_buf: std::collections::HashMap<u32, (String, String)> =
        std::collections::HashMap::new();
    // index -> id, for tool calls whose id arrived first (DeepSeek: real id in first chunk,
    // empty id in subsequent chunks).  Used to route continuation deltas correctly.
    let mut index_to_id: std::collections::HashMap<u32, String> = std::collections::HashMap::new();

    loop {
        // Check abort flag before polling the next event.
        if abort_flag.load(Ordering::Acquire) {
            // Emit aborted event, persist what we have, recover agent.
            {
                let mut map = state.chats.lock().unwrap();
                if let Some(entry) = map.get_mut(&conversation_id) {
                    entry.emit(json!({"type": "aborted"}).to_string());
                }
            }
            if !reply_buf.is_empty() {
                let msg = AgentMessage::new(Role::Assistant, &reply_buf);
                state.persist_message(conversation_id, &msg);
            }
            if let Some(recovered) = stream.into_agent() {
                let mut map = state.chats.lock().unwrap();
                if let Some(entry) = map.get_mut(&conversation_id) {
                    entry.generating = false;
                    entry.abort_flag.store(false, Ordering::Release);
                    entry.agent = Some(recovered);
                }
            }
            return None;
        }

        poll_count += 1;
        let t_poll = std::time::Instant::now();
        info!(conversation = %conversation_id, "[TIMING] poll #{poll_count}: calling stream.next()");

        tokio::select! {
            biased;

            agent_event = stream.next() => {
                let elapsed = t_poll.elapsed();
                let Some(event) = agent_event else {
                    info!(conversation = %conversation_id, "[TIMING] poll #{poll_count}: stream.next() -> None (stream ended) in {elapsed:?}");
                    break;
                };


                let payload = match event {
                    Ok(AgentEvent::Token(token)) => {
                        info!(conversation = %conversation_id, "[TIMING] poll #{poll_count}: stream.next() -> Token ({} chars) in {elapsed:?}", token.len());
                        reply_buf.push_str(&token);
                        json!({"type": "token", "content": token}).to_string()
                    }
                    Ok(AgentEvent::ToolCall(c)) => {
                        info!(conversation = %conversation_id, "[TIMING] poll #{poll_count}: stream.next() -> ToolCall name={} id={} index={} delta_len={} in {elapsed:?}", c.name, c.id, c.index, c.delta.len());
                        // Accumulate args by id, preserving insertion order.
                        // Both DeepSeek and MiniMax send the real id in the first chunk;
                        // subsequent chunks have empty id and are routed via index_to_id.
                        // pending_name_buf is a safety net for the hypothetical case where
                        // name/delta arrive before the id chunk.
                        if c.id.is_empty() {
                            // Empty id chunk — continuation delta, route via index_to_id.
                            if let Some(known_id) = index_to_id.get(&c.index).cloned() {
                                // DeepSeek continuation — append delta and emit immediately.
                                if let Some(entry) = pending_tools.get_mut(&known_id) {
                                    entry.1.push_str(&c.delta);
                                }
                                if c.delta.is_empty() {
                                    String::new()
                                } else {
                                    let name = pending_tools.get(&known_id)
                                        .map(|e| e.0.clone())
                                        .unwrap_or_default();
                                    json!({
                                        "type": "tool_call",
                                        "id": known_id,
                                        "name": name,
                                        "delta": c.delta,
                                    }).to_string()
                                }
                            } else {
                                // MiniMax: buffer until real id arrives
                                let entry = pending_name_buf.entry(c.index).or_insert_with(|| (c.name.clone(), String::new()));
                                if !c.name.is_empty() { entry.0 = c.name.clone(); }
                                entry.1.push_str(&c.delta);
                                String::new()
                            }
                        } else {
                            // Real id chunk — record index->id mapping for future empty-id chunks.
                            index_to_id.entry(c.index).or_insert_with(|| c.id.clone());
                            // Flush any buffered name/delta from empty-id chunks (MiniMax case)
                            let (buffered_name, buffered_delta) = pending_name_buf.remove(&c.index)
                                .unwrap_or_default();
                            let name = if pending_tools.contains_key(&c.id) {
                                c.name.clone()
                            } else if !buffered_name.is_empty() {
                                buffered_name
                            } else {
                                c.name.clone()
                            };
                            let order = &mut pending_tool_order;
                            let tools = &mut pending_tools;
                            let entry = tools.entry(c.id.clone()).or_insert_with(|| {
                                order.push(c.id.clone());
                                (name.clone(), String::new(), None)
                            });
                            // Prepend buffered delta, then current delta
                            let full_delta = format!("{}{}", buffered_delta, c.delta);
                            entry.1.push_str(&full_delta);
                            if full_delta.is_empty() {
                                String::new()
                            } else {
                                json!({
                                    "type": "tool_call",
                                    "id": c.id,
                                    "name": &entry.0,
                                    "delta": full_delta,
                                }).to_string()
                            }
                        }
                    }
                    Ok(AgentEvent::ToolResult(res)) => {
                        info!(conversation = %conversation_id, "[TIMING] poll #{poll_count}: stream.next() -> ToolResult name={} id={} in {elapsed:?}", res.name, res.id);
                        // Drain queued interrupts.
                        {
                            let mut map = state.chats.lock().unwrap();
                            if let Some(entry) = map.get_mut(&conversation_id) {
                                entry.queued_interrupts.clear();
                            }
                        }

                        {
                            use ds_api::raw::request::message::{FunctionCall, ToolCall, ToolType};

                            // Overwrite accumulated args with the complete args from ToolCallResult.
                            if let Some(entry) = pending_tools.get_mut(&res.id) {
                                entry.1 = res.args.clone();
                                let result_str = serde_json::to_string(&res.result).unwrap_or_default();
                                entry.2 = Some(result_str);
                            }

                            // Flush only when every pending tool call has received its result.
                            let all_done = !pending_tool_order.is_empty()
                                && pending_tool_order.iter()
                                    .all(|id| pending_tools.get(id).and_then(|e| e.2.as_ref()).is_some());

                            if all_done {
                                // One assistant message with all tool_calls for this turn.
                                let tool_calls: Vec<ToolCall> = pending_tool_order.iter()
                                    .filter_map(|id| {
                                        pending_tools.get(id).map(|(name, args, _)| ToolCall {
                                            id: id.clone(),
                                            r#type: ToolType::Function,
                                            function: FunctionCall {
                                                name: name.clone(),
                                                arguments: args.clone(),
                                            },
                                        })
                                    })
                                    .collect();

                                let assistant_msg = AgentMessage {
                                    role: Role::Assistant,
                                    content: if reply_buf.is_empty() { None } else { Some(reply_buf.clone()) },
                                    name: None,
                                    tool_call_id: None,
                                    tool_calls: Some(tool_calls),
                                    reasoning_content: if reasoning_buf.is_empty() { None } else { Some(reasoning_buf.clone()) },
                                    prefix: None,
                                };
                                state.persist_message_async(conversation_id, &assistant_msg).await;
                                reply_buf.clear();
                                reasoning_buf.clear();

                                // Persist each tool result in order.
                                for id in &pending_tool_order {
                                    if let Some((name, _, Some(result_str))) = pending_tools.get(id) {
                                        let tool_msg = AgentMessage {
                                            role: Role::Tool,
                                            content: Some(result_str.clone()),
                                            name: Some(name.clone()),
                                            tool_call_id: Some(id.clone()),
                                            tool_calls: None,
                                            reasoning_content: None,
                                            prefix: None,
                                        };
                                        state.persist_message_async(conversation_id, &tool_msg).await;
                                    }
                                }

                                pending_tools.clear();
                                pending_tool_order.clear();
                            }
                        }

                        json!({
                            "type": "tool_result",
                            "id": res.id,
                            "name": res.name,
                            "args": res.args,
                            "result": res.result,
                        }).to_string()
                    }
                    Ok(AgentEvent::ReasoningToken(token)) => {
                        info!(conversation = %conversation_id, "[TIMING] poll #{poll_count}: stream.next() -> ReasoningToken ({} chars) in {elapsed:?}", token.len());
                        reasoning_buf.push_str(&token);
                        json!({"type": "reasoning_token", "content": token}).to_string()
                    }
                    Err(e) => {
                        let err_msg = e.to_string();
                        error!(conversation = %conversation_id, "[TIMING] poll #{poll_count}: stream.next() -> Error in {elapsed:?}: {err_msg}");

                        // Some providers occasionally end an otherwise-complete turn with a
                        // benign stream-tail error (e.g. "Error in input stream"). If we already
                        // received assistant content, treat this as a graceful completion so the
                        // UI does not show a false warning banner.
                        let is_benign_tail_error = err_msg.contains("Error in input stream")
                            && !reply_buf.trim().is_empty();

                        if is_benign_tail_error {
                            warn!(conversation = %conversation_id, "Treating benign tail stream error as done: {err_msg}");
                            if !reply_buf.is_empty() {
                                let msg = AgentMessage::new(Role::Assistant, &reply_buf);
                                state.persist_message(conversation_id, &msg);
                            }

                            let recovered = stream.into_agent();
                            let queued_interrupt = {
                                let mut map = state.chats.lock().unwrap();
                                if let Some(entry) = map.get_mut(&conversation_id) {
                                    entry.emit(json!({"type": "done"}).to_string());
                                    entry.generating = false;
                                    if let Some(agent) = recovered {
                                        entry.agent = Some(agent);
                                    }
                                    entry.queued_interrupts.drain(..).next()
                                } else {
                                    None
                                }
                            };

                            return queued_interrupt;
                        }

                        let payload = json!({"type": "error", "message": err_msg}).to_string();
                        // Emit the error event before recovering.
                        {
                            let mut map = state.chats.lock().unwrap();
                            if let Some(entry) = map.get_mut(&conversation_id) {
                                entry.emit(payload);
                            }
                        }
                        // Recover the agent.
                        if let Some(recovered) = stream.into_agent() {
                            let mut map = state.chats.lock().unwrap();
                            if let Some(entry) = map.get_mut(&conversation_id) {
                                entry.generating = false;
                                entry.agent = Some(recovered);
                            }
                        }
                        return None;
                    }
                };

                // Emit to log + broadcast.
                if !payload.is_empty() {
                    let mut map = state.chats.lock().unwrap();
                    if let Some(entry) = map.get_mut(&conversation_id) {
                        entry.emit(payload);
                    }
                }
            }

            else => {
                info!(conversation = %conversation_id, "[TIMING] poll #{poll_count}: select! else branch triggered in {:?}", t_poll.elapsed());
                break;
            }
        }
    }

    // ── Recover agent ─────────────────────────────────────────────────────────
    let recovered = stream.into_agent();

    // ── Persist assistant reply ───────────────────────────────────────────────
    if !reply_buf.is_empty() {
        let msg = AgentMessage::new(Role::Assistant, &reply_buf);
        state.persist_message(conversation_id, &msg);
    }

    // ── Emit done, put agent back ─────────────────────────────────────────────
    // Returns Some(text) if an interrupt arrived during the final turn (no
    // tool calls consumed it); generation_loop will start a new turn for it.
    {
        let mut map = state.chats.lock().unwrap();
        if let Some(entry) = map.get_mut(&conversation_id) {
            entry.emit(json!({"type": "done"}).to_string());
            entry.generating = false;
            if let Some(agent) = recovered {
                entry.agent = Some(agent);
            }
            // Drain any queued interrupts that arrived during the final turn.
            entry.queued_interrupts.drain(..).next()
        } else {
            None
        }
    }
}
