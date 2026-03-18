use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::config::{Config, McpServerConfig, ModelConfig};
use crate::db::{Db, to_vector};
use crate::embedding::EmbeddingClient;
use crate::spells::{SpellDeps, build_all_spells};
use ds_api::AgentEvent;
use ds_api::DeepseekAgent;
use ds_api::McpTool;
use ds_api::Tool as _;
use ds_api::ToolInjection;
use serde_json::Value;
use sqlx::PgPool;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::broadcast;
use tokio::sync::mpsc::UnboundedSender;
use tracing::{error, info, warn};
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
    pub agent: Option<DeepseekAgent>,
    pub interrupt_tx: UnboundedSender<String>,
    pub tool_inject_tx: UnboundedSender<ToolInjection>,
    pub user_mcp_tools: Arc<tokio::sync::Mutex<Vec<(String, McpTool)>>>,
    pub broadcast_tx: broadcast::Sender<Arc<WsEvent>>,
    pub event_log: Vec<Arc<WsEvent>>,
    pub generating: bool,
    pub abort_flag: Arc<AtomicBool>,
    pub ask_user_pending: Arc<tokio::sync::Mutex<Option<tokio::sync::oneshot::Sender<String>>>>,
    pub queued_interrupts: Vec<String>,
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

    pub fn emit(&mut self, payload: String) {
        let seq = NEXT_SEQ.fetch_add(1, AtomicOrdering::Relaxed);
        let ev = Arc::new(WsEvent { seq, payload });
        if self.event_log.len() >= EVENT_LOG_CAP {
            self.event_log.remove(0);
        }
        self.event_log.push(Arc::clone(&ev));
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

    pub async fn build_agent(
        &self,
        conversation_id: Uuid,
    ) -> (
        DeepseekAgent,
        UnboundedSender<String>,
        Arc<tokio::sync::Mutex<Option<tokio::sync::oneshot::Sender<String>>>>,
        tokio::sync::mpsc::UnboundedSender<ToolInjection>,
        Arc<tokio::sync::Mutex<Vec<(String, McpTool)>>>,
        tokio::sync::broadcast::Sender<String>,
        Arc<AtomicBool>,
    ) {
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

        let ask_user_pending = Arc::new(tokio::sync::Mutex::new(None));
        let abort_flag = Arc::new(AtomicBool::new(false));

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
                .unwrap_or_default();

        let user_mcp_rows: Vec<(String, String, serde_json::Value)> = sqlx::query_as(
            r#"SELECT name, "type", config FROM user_mcps WHERE user_id = $1 ORDER BY created_at ASC"#
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default();

        let (spawn_tx, _) = tokio::sync::broadcast::channel::<String>(256);

        let user_settings: Option<(Option<Value>, Option<Value>, Option<String>)> = sqlx::query_as(
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

        let user_mcp_tools: Arc<tokio::sync::Mutex<Vec<(String, McpTool)>>> =
            Arc::new(tokio::sync::Mutex::new(Vec::new()));

        let spell_deps = SpellDeps {
            subagent_prompt: global_cfg.subagent_prompt(),
            ask_pending: Arc::clone(&ask_user_pending),
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
            tool_inject_tx: inject_tx.clone(),
            pool: self.pool.clone(),
            user_id,
            sandbox: self.sandbox.clone(),
            mcp_catalog: global_cfg.mcp_catalog.clone(),
            abort_flag: Arc::clone(&abort_flag),
        };

        let mut agent = base.add_tool(build_all_spells(spell_deps));

        for (_, tool) in mcp_snapshot {
            agent = agent.add_tool(tool);
        }

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
                    let (cmd, args) = self.sandbox.wrap_mcp_command(user_id, &command, &args_ref);
                    let args_wrapped: Vec<&str> = args.iter().map(String::as_str).collect();

                    match tokio::time::timeout(
                        Duration::from_secs(300),
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
                            warn!("user MCP '{name}' startup timed out (300s), skipping");
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
                let log = entry.event_log.clone();
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
                .unwrap_or_default();

        let (agent, tx, ask_user_pending, inject_tx, user_mcp_tools, spawn_tx, abort_flag) =
            self.build_agent(conversation_id).await;

        let mut rx = spawn_tx.subscribe();
        let relay_state = self.clone();
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
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break, // 通道废弃时干净地退出，不留后患
                }
            }
        });

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

    pub async fn start_generation(&self, conversation_id: Uuid, user_text: String) -> bool {
        let (agent, abort_flag) = {
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
            (entry.agent.take(), Arc::clone(&entry.abort_flag))
        };

        let mut agent = match agent {
            Some(a) => a,
            None => {
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
                    entry.spawn_tx = fresh_spawn_tx.clone();
                    entry.abort_flag = Arc::clone(&fresh_abort);
                }

                let mut rx = fresh_spawn_tx.subscribe();
                let relay_state = self.clone();
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

                fresh_agent
            }
        };

        agent.push_user_message_with_name(&user_text, None);

        let state = self.clone();

        tokio::spawn(async move {
            generation_loop(state, conversation_id, agent, abort_flag).await;
        });

        true
    }

    pub fn send_interrupt(&self, conversation_id: Uuid, content: String) {
        let mut map = self.chats.lock().unwrap();
        if let Some(entry) = map.get_mut(&conversation_id) {
            let _ = entry.interrupt_tx.send(content.clone());
            entry.queued_interrupts.push(content);
        }
    }

    pub fn abort_generation(&self, conversation_id: Uuid) {
        let map = self.chats.lock().unwrap();
        if let Some(entry) = map.get(&conversation_id) {
            entry.abort_flag.store(true, Ordering::Release);
        }
    }

    pub async fn persist_message_async(
        &self,
        conversation_id: Uuid,
        msg: &ds_api::raw::request::message::Message,
    ) {
        let db = self.db.clone();
        let msg = msg.clone();
        let state = self.clone();

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
                let global_cfg = state.get_global_config().await.unwrap_or_default();
                let embed = EmbeddingClient::new(
                    global_cfg.embedding.api_key,
                    global_cfg.embedding.api_base,
                    global_cfg.embedding.name,
                );

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

    pub fn persist_message(
        &self,
        conversation_id: Uuid,
        msg: &ds_api::raw::request::message::Message,
    ) {
        let db = self.db.clone();
        let msg = msg.clone();
        let state = self.clone();

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
                let global_cfg = state.get_global_config().await.unwrap_or_default();
                let embed = EmbeddingClient::new(
                    global_cfg.embedding.api_key,
                    global_cfg.embedding.api_base,
                    global_cfg.embedding.name,
                );

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

    let mut pending_tools: std::collections::HashMap<String, (String, String, Option<String>)> =
        std::collections::HashMap::new();
    let mut pending_tool_order: Vec<String> = Vec::new();
    let mut pending_name_buf: std::collections::HashMap<u32, (String, String)> =
        std::collections::HashMap::new();
    let mut index_to_id: std::collections::HashMap<u32, String> = std::collections::HashMap::new();

    loop {
        if abort_flag.load(Ordering::Acquire) {
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

                        if c.id.is_empty() {
                            if let Some(known_id) = index_to_id.get(&c.index).cloned() {
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
                                let entry = pending_name_buf.entry(c.index).or_insert_with(|| (c.name.clone(), String::new()));
                                if !c.name.is_empty() { entry.0 = c.name.clone(); }
                                entry.1.push_str(&c.delta);
                                String::new()
                            }
                        } else {
                            index_to_id.entry(c.index).or_insert_with(|| c.id.clone());
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

                        {
                            let mut map = state.chats.lock().unwrap();
                            if let Some(entry) = map.get_mut(&conversation_id) {
                                entry.queued_interrupts.clear();
                            }
                        }

                        {
                            use ds_api::raw::request::message::{FunctionCall, ToolCall, ToolType};

                            if let Some(entry) = pending_tools.get_mut(&res.id) {
                                entry.1 = res.args.clone();
                                let result_str = serde_json::to_string(&res.result).unwrap_or_default();
                                entry.2 = Some(result_str);
                            }

                            let all_done = !pending_tool_order.is_empty()
                                && pending_tool_order.iter()
                                    .all(|id| pending_tools.get(id).and_then(|e| e.2.as_ref()).is_some());

                            if all_done {
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

                        {
                            let mut map = state.chats.lock().unwrap();
                            if let Some(entry) = map.get_mut(&conversation_id) {
                                entry.emit(payload);
                            }
                        }

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

    let recovered = stream.into_agent();

    if !reply_buf.is_empty() {
        let msg = AgentMessage::new(Role::Assistant, &reply_buf);
        state.persist_message(conversation_id, &msg);
    }

    {
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
    }
}
