use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::config::{Config, McpServerConfig};
use ds_api::AgentEvent;
use ds_api::DeepseekAgent;
use ds_api::McpTool;
use ds_api::Tool as _;
use sqlx::PgPool;
use tokio::sync::broadcast;
use tokio::sync::mpsc::UnboundedSender;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::db::{Db, to_vector};
use crate::embedding::EmbeddingClient;
use crate::spells::{
    A2aSpell, AskUserSpell, CommandSpell, FileSpell, HistorySpell, ManageMcpSpell, OutlineSpell,
    PresentFileSpell, ScriptSpell, SearchSpell,
};
use std::sync::atomic::{AtomicBool, Ordering};

// How many events to keep in the log for late-joining clients.
// Enough to replay a full long turn including many tool calls.
const EVENT_LOG_CAP: usize = 4096;

// broadcast channel capacity — how many events can be buffered before
// a slow subscriber starts missing messages.
const BROADCAST_CAP: usize = 256;

/// A single event that was (or will be) sent over WebSocket.
/// Stored in the event log so late-joining clients can replay.
#[derive(Debug, Clone)]
pub struct WsEvent {
    pub payload: String, // serialised JSON, ready to send
}

/// One entry per conversation.
pub struct ChatEntry {
    /// The agent when idle (not generating). Taken out during generation.
    pub agent: Option<DeepseekAgent>,

    /// Kept alive so the agent's interrupt receiver is never dropped.
    pub interrupt_tx: UnboundedSender<String>,

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

    /// Set to true by ManageMcpSpell after install/uninstall. The generation
    /// cleanup will drop the recovered agent instead of restoring it, so the
    /// next start_generation rebuilds with the updated MCP tool list.
    pub agent_stale: Arc<AtomicBool>,

    /// Interrupt messages that haven't been consumed by the agent yet.
    /// Populated by send_interrupt alongside the agent's internal channel.
    /// Cleared after each ToolResult (the agent drains its channel then).
    /// Any messages remaining after "done" are processed as a new generation.
    pub queued_interrupts: Vec<String>,
}

impl ChatEntry {
    fn new(
        agent: DeepseekAgent,
        interrupt_tx: UnboundedSender<String>,
        ask_user_pending: Arc<tokio::sync::Mutex<Option<tokio::sync::oneshot::Sender<String>>>>,
        agent_stale: Arc<AtomicBool>,
    ) -> Self {
        let (broadcast_tx, _) = broadcast::channel(BROADCAST_CAP);
        Self {
            agent: Some(agent),
            interrupt_tx,
            broadcast_tx,
            event_log: Vec::new(),
            generating: false,
            abort_flag: Arc::new(AtomicBool::new(false)),
            ask_user_pending,
            agent_stale,
            queued_interrupts: Vec::new(),
        }
    }

    /// Emit an event: append to the log and broadcast to all live subscribers.
    /// The log is capped at EVENT_LOG_CAP to avoid unbounded memory growth.
    pub fn emit(&mut self, payload: String) {
        let ev = Arc::new(WsEvent { payload });
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
#[derive(Clone)]
pub struct AppState {
    /// Per-conversation agent instances, keyed by conversation UUID.
    pub chats: Arc<Mutex<HashMap<Uuid, ChatEntry>>>,

    /// DeepSeek API key, used when constructing new agents.
    pub deepseek_token: String,

    /// Base URL for the DeepSeek-compatible API.
    pub model_api_base: String,

    /// Model name to use for every agent turn.
    pub model_name: String,

    /// Optional system prompt applied to every freshly created agent.
    pub system_prompt: Option<String>,

    /// PostgreSQL connection pool.
    pub pool: PgPool,

    /// Thin wrapper around the pool for message persistence.
    pub db: Db,

    /// Embedding client — shared across all conversations.
    pub embed: EmbeddingClient,

    /// Named MCP tools — started at startup and updated dynamically by
    /// ManageMcpSpell. Wrapped in Arc<Mutex> so the spell can add/remove
    /// entries without going through AppState.
    pub mcp_tools: Arc<tokio::sync::Mutex<Vec<(String, McpTool)>>>,

    /// Number of raw tool definitions registered by the built-in spells
    /// (computed once at startup). Used by ManageMcpSpell to enforce max_tools.
    pub builtin_tool_count: usize,

    /// Maximum total tool definitions (built-in + all MCP). From config.
    pub max_tools: usize,
}

impl AppState {
    pub fn new(
        cfg: &Config,
        pool: PgPool,
        mcp_tools: Vec<(String, McpTool)>,
        builtin_tool_count: usize,
    ) -> Self {
        let db = Db::new(pool.clone());
        Self {
            chats: Arc::new(Mutex::new(HashMap::new())),
            deepseek_token: cfg.model.api_key.clone(),
            model_api_base: cfg.model.api_base.clone(),
            model_name: cfg.model.name.clone(),
            system_prompt: cfg.system_prompt(),
            pool,
            db,
            embed: EmbeddingClient::new(
                cfg.embedding.api_key.clone(),
                cfg.embedding.api_base.clone(),
                cfg.embedding.name.clone(),
            ),
            mcp_tools: Arc::new(tokio::sync::Mutex::new(mcp_tools)),
            builtin_tool_count,
            max_tools: cfg.limits.max_tools,
        }
    }

    /// Initialise MCP servers from config. Called once at startup.
    /// Failures are logged and skipped — a missing MCP server should never
    /// prevent familiar from starting.
    pub async fn init_mcp(mcp_configs: &[McpServerConfig]) -> Vec<(String, McpTool)> {
        let mut tools = Vec::new();

        for mc in mcp_configs {
            let args: Vec<&str> = mc.args.iter().map(|s| s.as_str()).collect();
            match McpTool::stdio(&mc.command, &args).await {
                Ok(t) => {
                    info!("MCP: {} ready ({} tools)", mc.name, t.raw_tools().len());
                    tools.push((mc.name.clone(), t));
                }
                Err(e) => warn!("MCP: {} failed to start: {e}", mc.name),
            }
        }

        tools
    }

    /// Build a fresh agent for `conversation_id`, restoring history from PG.
    /// Returns `(agent, interrupt_tx, ask_user_pending, agent_stale)`.
    pub async fn build_agent(
        &self,
        conversation_id: Uuid,
    ) -> (
        DeepseekAgent,
        UnboundedSender<String>,
        Arc<tokio::sync::Mutex<Option<tokio::sync::oneshot::Sender<String>>>>,
        Arc<AtomicBool>,
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
        let agent_stale = Arc::new(AtomicBool::new(false));

        // Snapshot the MCP tools BEFORE building the agent so the builder
        // is never in scope across an await point. This keeps the generated
        // future Send even when the builder or tool types are !Send.
        let mcp_snapshot: Vec<_> = {
            let guard = self.mcp_tools.lock().await;
            guard.iter().cloned().collect()
        };

        let mut builder = DeepseekAgent::custom(
            self.deepseek_token.clone(),
            self.model_api_base.clone(),
            self.model_name.clone(),
        )
        .with_streaming()
        .with_history(history)
        .add_tool(CommandSpell)
        .add_tool(FileSpell)
        .add_tool(ScriptSpell)
        .add_tool(PresentFileSpell)
        .add_tool(A2aSpell)
        .add_tool(SearchSpell)
        .add_tool(OutlineSpell)
        .add_tool(AskUserSpell {
            pending: Arc::clone(&ask_user_pending),
        })
        .add_tool(ManageMcpSpell {
            mcp_tools: Arc::clone(&self.mcp_tools),
            agent_stale: Arc::clone(&agent_stale),
            builtin_tool_count: self.builtin_tool_count,
            max_tools: self.max_tools,
        })
        .add_tool(HistorySpell {
            db: self.db.clone(),
            embed: self.embed.clone(),
            conversation_id,
        });

        for (_, tool) in mcp_snapshot {
            builder = builder.add_tool(tool);
        }

        if let Some(prompt) = &self.system_prompt {
            builder = builder.with_system_prompt(prompt.clone());
        }

        let (agent, tx) = builder.with_interrupt_channel();
        (agent, tx, ask_user_pending, agent_stale)
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
        let (agent, tx, ask_user_pending, agent_stale) = self.build_agent(conversation_id).await;
        let entry = ChatEntry::new(agent, tx, ask_user_pending, agent_stale);
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
        let (agent, abort_flag, agent_stale) = {
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
            (
                entry.agent.take(),
                Arc::clone(&entry.abort_flag),
                Arc::clone(&entry.agent_stale),
            )
        };

        let mut agent = match agent {
            Some(a) => a,
            None => {
                // Agent was dropped because MCP tools changed — rebuild outside lock.
                let (fresh_agent, fresh_tx, fresh_pending, fresh_stale) =
                    self.build_agent(conversation_id).await;
                let mut map = self.chats.lock().unwrap();
                if let Some(entry) = map.get_mut(&conversation_id) {
                    // Swap in the new interrupt channel and stale flag.
                    entry.interrupt_tx = fresh_tx;
                    entry.ask_user_pending = fresh_pending;
                    entry.agent_stale = Arc::clone(&fresh_stale);
                }
                fresh_agent
            }
        };

        agent.push_user_message_with_name(&user_text, None);

        let state = self.clone();

        tokio::spawn(async move {
            generation_loop(state, conversation_id, agent, abort_flag, agent_stale).await;
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
    initial_stale: Arc<AtomicBool>,
) {
    let mut agent = initial_agent;
    let mut abort_flag = initial_abort;
    let mut agent_stale = initial_stale;

    loop {
        let pending_text = run_generation(
            state.clone(),
            conversation_id,
            agent,
            abort_flag,
            agent_stale,
        )
        .await;

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
                let stale = Arc::clone(&entry.agent_stale);
                Some((entry.agent.take(), abort, stale))
            } else {
                None
            }
        };

        let (agent_opt, new_abort, mut new_stale) = match next {
            Some(t) => t,
            None => break,
        };

        let next_agent = match agent_opt {
            Some(a) => a,
            None => {
                // Agent was stale; rebuild from history.
                let (a, tx, pend, s) = state.build_agent(conversation_id).await;
                {
                    let mut map = state.chats.lock().unwrap();
                    if let Some(entry) = map.get_mut(&conversation_id) {
                        entry.interrupt_tx = tx;
                        entry.ask_user_pending = pend;
                        entry.agent_stale = Arc::clone(&s);
                    }
                }
                new_stale = s;
                a
            }
        };

        let mut next_agent = next_agent;
        next_agent.push_user_message_with_name(&text, None);

        agent = next_agent;
        abort_flag = new_abort;
        agent_stale = new_stale;
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
    agent_stale: Arc<AtomicBool>,
) -> Option<String> {
    use ds_api::raw::request::message::{Message as AgentMessage, Role};
    use futures::StreamExt;
    use serde_json::json;

    info!(conversation = %conversation_id, "[TIMING] run_generation started, calling chat_from_history");
    let t_start = std::time::Instant::now();
    let mut stream = agent.chat_from_history();
    info!(conversation = %conversation_id, "[TIMING] chat_from_history returned in {:?}", t_start.elapsed());
    let mut reply_buf = String::new();
    let mut poll_count = 0u32;

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
                    // If MCP tools changed, discard the agent so the next turn
                    // rebuilds it with the updated tool list.
                    if !agent_stale.load(Ordering::Relaxed) {
                        entry.agent = Some(recovered);
                    }
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
                        info!(conversation = %conversation_id, "[TIMING] poll #{poll_count}: stream.next() -> ToolCall name={} id={} delta_len={} in {elapsed:?}", c.name, c.id, c.delta.len());
                        json!({
                            "type": "tool_call",
                            "id": c.id,
                            "name": c.name,
                            "delta": c.delta,
                        }).to_string()
                    }
                    Ok(AgentEvent::ToolResult(res)) => {
                        info!(conversation = %conversation_id, "[TIMING] poll #{poll_count}: stream.next() -> ToolResult name={} id={} in {elapsed:?}", res.name, res.id);
                        // The agent will drain its interrupt channel before the next
                        // sampling turn. Clear our queue so we don't double-process
                        // any interrupts that were consumed mid-generation.
                        {
                            let mut map = state.chats.lock().unwrap();
                            if let Some(entry) = map.get_mut(&conversation_id) {
                                entry.queued_interrupts.clear();
                            }
                        }
                        json!({
                            "type": "tool_result",
                            "id": res.id,
                            "name": res.name,
                            "result": res.result,
                        }).to_string()
                    }
                    Ok(AgentEvent::ReasoningToken(token)) => {
                        info!(conversation = %conversation_id, "[TIMING] poll #{poll_count}: stream.next() -> ReasoningToken ({} chars) in {elapsed:?}", token.len());
                        json!({"type": "reasoning_token", "content": token}).to_string()
                    }
                    Err(e) => {
                        error!(conversation = %conversation_id, "[TIMING] poll #{poll_count}: stream.next() -> Error in {elapsed:?}: {e}");
                        let payload = json!({"type": "error", "message": e.to_string()}).to_string();
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
                                if !agent_stale.load(Ordering::Relaxed) {
                                    entry.agent = Some(recovered);
                                }
                            }
                        }
                        return None;
                    }
                };

                // Emit to log + broadcast.
                {
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
            // Only restore the agent if the MCP tool list hasn't changed.
            if !agent_stale.load(Ordering::Relaxed)
                && let Some(agent) = recovered
            {
                entry.agent = Some(agent);
            }
            // Drain any queued interrupts that arrived during the final turn.
            entry.queued_interrupts.drain(..).next()
        } else {
            None
        }
    }
}
