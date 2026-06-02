//! Background generation worker.
//!
//! A `Worker` is spawned per user message. It:
//! 1. Loads conversation history from DB
//! 2. Connects all MCPs (global + user + conversation) from DB on the fly
//! 3. Builds a `ToolBundle` (spells + MCPs + tunnel)
//! 4. Runs the LLM ↔ tool-call loop via `LlmClient::stream()`
//! 5. Writes every SSE-worthy event to `generation_events` (+ pg_notify)
//! 6. Persists assistant / tool-result messages to the messages table
//!
//! There is **zero in-memory state** carried between generations.

use std::sync::Arc;
use std::time::Duration;

use agentix::raw::shared::ToolDefinition;
use agentix::types::UsageStats;
use agentix::{LlmEvent, McpTool, Message, Request, Tool, ToolBundle, ToolOutput, UserContent};
use futures::StreamExt;
use serde_json::{Value, json};
use sqlx::PgPool;
use tokio::time::timeout;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::config::{Config, ModelConfig};
use crate::db::{Db, to_vector};
use crate::embedding::EmbeddingClient;
use crate::sandbox::SandboxManager;
use crate::spells::{SpellDeps, build_all_spells};
use crate::web::tunnel::TunnelRegistry;

// ── Public API ────────────────────────────────────────────────────────────────

/// Everything a worker needs — purely derived from DB + config at call time.
#[derive(Clone)]
pub struct WorkerContext {
    pub job_id: Uuid,
    pub conversation_id: Uuid,
    pub user_id: Uuid,
    pub pool: PgPool,
    pub db: Db,
    pub sandbox: Arc<SandboxManager>,
    pub tunnel_registry: TunnelRegistry,
}

/// Spawn a background generation worker for the given job.
/// Returns immediately. The worker runs in a detached tokio task.
pub fn spawn_worker(ctx: WorkerContext) {
    tokio::spawn(async move {
        if let Err(e) = run_worker(ctx).await {
            error!("worker failed: {e}");
        }
    });
}

// ── Core worker logic ─────────────────────────────────────────────────────────

async fn run_worker(ctx: WorkerContext) -> anyhow::Result<()> {
    // Mark job as running.
    sqlx::query("UPDATE generation_jobs SET status = 'running', updated_at = now() WHERE id = $1")
        .bind(ctx.job_id)
        .execute(&ctx.pool)
        .await?;

    let t_start = std::time::Instant::now();
    let result = run_worker_inner(&ctx).await;
    let duration_ms = t_start.elapsed().as_millis() as i64;

    match &result {
        Ok(()) => {
            emit(&ctx, json!({"type": "done"})).await;
            record_job_latency(&ctx.pool, ctx.job_id, None, Some(duration_ms), None, None).await;
            set_job_status(&ctx.pool, ctx.job_id, "done", None).await;
        }
        Err(e) => {
            let msg = e.to_string();
            emit(&ctx, json!({"type": "error", "message": &msg})).await;
            record_job_latency(&ctx.pool, ctx.job_id, None, Some(duration_ms), None, None).await;
            set_job_status(&ctx.pool, ctx.job_id, "error", Some(&msg)).await;
        }
    }

    // MCP stdio processes and sandbox shell commands for this generation all
    // run against the per-conversation container. Once the worker is done, we
    // can drop that container and recreate it on demand next turn.
    // ctx.sandbox.remove_container(ctx.conversation_id);

    result
}

async fn run_worker_inner(ctx: &WorkerContext) -> anyhow::Result<()> {
    let t0 = std::time::Instant::now();
    let global_cfg = Config::load_from_db(&ctx.pool).await.unwrap_or_default();
    info!(ms = t0.elapsed().as_millis(), "⏱ load_from_db");

    // ── Resolve cheap model + system prompt from user settings ────────────
    let user_settings: Option<(Option<Value>, Option<String>)> =
        sqlx::query_as("SELECT cheap_model, system_prompt FROM user_settings WHERE user_id = $1")
            .bind(ctx.user_id)
            .fetch_optional(&ctx.pool)
            .await
            .unwrap_or(None);

    let cheap_cfg = if let Some((c, _)) = &user_settings {
        c.as_ref()
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_else(|| global_cfg.cheap_model.clone())
    } else {
        global_cfg.cheap_model.clone()
    };

    // User may supply a fully custom system prompt that bypasses PromptEngine.
    let custom_system_prompt: Option<String> = user_settings.and_then(|(_, p)| p);

    let current_time = chrono::Utc::now().to_rfc3339();
    // Tera is Send+Sync, so PromptEngine can live across .await points.
    let prompt_engine = crate::prompt::PromptEngine::new();

    // ── Resolve frontier model: conversation model_id > global default ────
    // Also tracks the resolved model's UUID (None when we fell back to
    // cheap_cfg, which has no models row), used to tag token_usage_events
    // so cost queries can JOIN per-model prices.
    let (frontier_cfg, frontier_model_id): (ModelConfig, Option<Uuid>) = {
        #[allow(clippy::too_many_arguments)]
        fn model_from_row(
            provider: String,
            name: String,
            api_base: String,
            api_key: String,
            extra_body: Value,
            kind: String,
            compact_trigger_tokens: i64,
            compact_tail_tokens: i64,
            reasoning_effort: Option<String>,
        ) -> ModelConfig {
            let provider_parsed = serde_json::from_value::<crate::config::Provider>(
                serde_json::Value::String(provider.clone()),
            )
            .unwrap_or_else(|_| panic!("unknown provider in DB: {provider}"));
            ModelConfig {
                provider: provider_parsed,
                name,
                api_base,
                api_key,
                extra_body: extra_body
                    .as_object()
                    .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                    .unwrap_or_default(),
                max_tokens: None,
                kind,
                compact_trigger_tokens,
                compact_tail_tokens,
                reasoning_effort: crate::config::parse_reasoning_effort(
                    reasoning_effort.as_deref(),
                ),
            }
        }

        // 1. conversation-level model_id
        // Defense-in-depth: silently ignore models not available to the
        // conversation owner. The UI filter + create_conversation guard should
        // catch this upstream; here we fall through to the default-model branch
        // so the job still completes rather than 500-ing.
        let conv_model: Option<(
            Uuid,
            String,
            String,
            String,
            String,
            Value,
            String,
            i64,
            i64,
            Option<String>,
        )> = sqlx::query_as(
            "SELECT m.id, m.provider, m.model_name, m.api_base, m.api_key, m.extra_body, m.kind,
                    m.compact_trigger_tokens, m.compact_tail_tokens, m.reasoning_effort
             FROM conversations c
             JOIN models m ON m.id = c.model_id
             WHERE c.id = $1
               AND (
                    (m.scope = 'user' AND m.user_id = c.user_id)
                    OR (
                        m.scope = 'global'
                        AND COALESCE(
                            (
                                SELECT allowed
                                FROM user_model_permissions ump
                                WHERE ump.user_id = c.user_id AND ump.model_id = m.id
                            ),
                            m.initial_available
                        )
                    )
               )",
        )
        .bind(ctx.conversation_id)
        .fetch_optional(&ctx.pool)
        .await
        .unwrap_or(None);

        if let Some((id, provider, name, api_base, api_key, extra_body, kind, trig, tail, effort)) =
            conv_model
        {
            (
                model_from_row(
                    provider, name, api_base, api_key, extra_body, kind, trig, tail, effort,
                ),
                Some(id),
            )
        } else {
            // 2. global default model
            let default_model: Option<(
                Uuid,
                String,
                String,
                String,
                String,
                Value,
                String,
                i64,
                i64,
                Option<String>,
            )> = sqlx::query_as(
                "SELECT id, provider, model_name, api_base, api_key, extra_body, kind,
                        compact_trigger_tokens, compact_tail_tokens, reasoning_effort
                 FROM models
                 WHERE scope = 'global'
                   AND is_default = true
                   AND COALESCE(
                        (
                            SELECT allowed
                            FROM user_model_permissions ump
                            WHERE ump.user_id = $1 AND ump.model_id = models.id
                        ),
                        initial_available
                   )
                 LIMIT 1",
            )
            .bind(ctx.user_id)
            .fetch_optional(&ctx.pool)
            .await
            .unwrap_or(None);

            if let Some((
                id,
                provider,
                name,
                api_base,
                api_key,
                extra_body,
                kind,
                trig,
                tail,
                effort,
            )) = default_model
            {
                (
                    model_from_row(
                        provider, name, api_base, api_key, extra_body, kind, trig, tail, effort,
                    ),
                    Some(id),
                )
            } else {
                // 3. fallback: cheap_model — no models row, so no price.
                (cheap_cfg.clone(), None)
            }
        }
    };

    // ── Record model + provider for this job ─────────────────────────────
    record_job_latency(
        &ctx.pool,
        ctx.job_id,
        None,
        None,
        Some(&frontier_cfg.name),
        Some(&format!("{:?}", frontier_cfg.provider).to_lowercase()),
    )
    .await;

    // ── Resolve user name ─────────────────────────────────────────────────
    let user_name: String = sqlx::query_scalar::<_, String>("SELECT name FROM users WHERE id = $1")
        .bind(ctx.user_id)
        .fetch_optional(&ctx.pool)
        .await
        .unwrap_or(None)
        .unwrap_or_default();

    // ── Build base system prompt via PromptEngine or custom override ───────
    let mut system_prompt: String = if let Some(ref custom) = custom_system_prompt {
        // User has provided a fully custom system prompt — use it as-is.
        crate::prompt_template::render_prompt(custom, &[("USER_NAME", &user_name)])
    } else {
        let raw = prompt_engine.build_main(false);
        crate::prompt_template::render_prompt(&raw, &[("USER_NAME", &user_name)])
    };

    // ── Append skills ─────────────────────────────────────────────────────
    // Start with bundled skills (compiled into the binary).
    let mut skill_map: std::collections::HashMap<String, String> = crate::prompt::BUNDLED_SKILLS
        .iter()
        .map(|(name, desc, _)| (name.to_string(), desc.to_string()))
        .collect();

    // DB app_skills override bundled (same name = DB wins).
    let app_skill_rows: Vec<(String, Option<String>)> =
        sqlx::query_as("SELECT name, description FROM app_skills ORDER BY name ASC")
            .fetch_all(&ctx.pool)
            .await
            .unwrap_or_default();
    for (name, desc) in app_skill_rows {
        skill_map.insert(name, desc.unwrap_or_default());
    }

    // User-private skills override everything.
    let user_skill_rows: Vec<(String, Option<String>)> = sqlx::query_as(
        "SELECT name, description FROM user_skills WHERE user_id = $1 ORDER BY name ASC",
    )
    .bind(ctx.user_id)
    .fetch_all(&ctx.pool)
    .await
    .unwrap_or_default();
    for (name, desc) in user_skill_rows {
        skill_map.insert(name, desc.unwrap_or_default());
    }

    let mut skills: Vec<String> = skill_map
        .into_iter()
        .map(|(name, desc)| {
            if desc.is_empty() {
                format!("- {name}")
            } else {
                format!("- {name}: {desc}")
            }
        })
        .collect();

    if !skills.is_empty() {
        skills.sort();
        skills.dedup();
        system_prompt.push_str(&format!(
            "\n\n可用 Skills（需要时调用 load_skill 获取详细指令）：\n{}",
            skills.join("\n")
        ));
    }

    // ── Load history from DB (summary + recent tail, transparently) ──────
    // Anchor-first: returns [nearest-anchor summary + raw tail] when the
    // active path carries a summary, else the raw path. Only a DB restore
    // failure errors here (propagated to the top-level error event);
    // oversized raw history is rejected with an error in the loop below.
    let t_restore = std::time::Instant::now();
    let messages = crate::compact::load_for_generation(
        &ctx.db,
        ctx.conversation_id,
        ctx.user_id,
    )
    .await
    .map_err(|e| {
        error!(conversation = %ctx.conversation_id, "failed to restore history: {e:#}");
        e
    })?;
    let mut messages = sanitize_history(messages);
    info!(conversation = %ctx.conversation_id, messages = messages.len(), ms = t_restore.elapsed().as_millis(), "⏱ restore history");

    // ── Connect MCPs from DB ──────────────────────────────────────────────
    let t_mcp = std::time::Instant::now();
    let mcp_tools = connect_mcps_from_db(ctx).await;
    info!(
        ms = t_mcp.elapsed().as_millis(),
        tools = mcp_tools.len(),
        "⏱ connect_mcps"
    );
    info!(ms = t0.elapsed().as_millis(), "⏱ total pre-LLM setup");

    // ── Build ToolBundle (spells + MCPs + tunnel) ─────────────────────────
    let spell_deps = SpellDeps {
        prompt_engine: crate::prompt::PromptEngine::new(),
        current_date: current_time.clone(),
        cheap_model: cheap_cfg.clone(),
        history: messages.clone(),
        db: ctx.db.clone(),
        embed: EmbeddingClient::new(
            global_cfg.embedding.api_key.clone(),
            global_cfg.embedding.api_base.clone(),
            global_cfg.embedding.name.clone(),
        ),
        conversation_id: ctx.conversation_id,
        pool: ctx.pool.clone(),
        user_id: ctx.user_id,
        sandbox: ctx.sandbox.clone(),
        mcp_catalog: global_cfg.mcp_catalog.clone(),
        tavily_api_key: global_cfg.tavily_api_key.clone(),
        siliconflow_api_key: global_cfg.siliconflow_api_key.clone(),
        fal_api_key: global_cfg.fal_api_key.clone(),
        http: reqwest::Client::new(),
    };

    let mut bundle = ToolBundle::new();
    bundle.push(build_all_spells(spell_deps));
    for (_, tool) in &mcp_tools {
        bundle.push(tool.clone());
    }

    // Tunnel tools (live WebSocket — only in-memory source)
    {
        let registry = ctx.tunnel_registry.lock().await;
        if let Some(tunnel_tools) = registry.get(&ctx.user_id) {
            for tunnel_tool in tunnel_tools {
                info!(
                    user = %ctx.user_id,
                    tools = tunnel_tool.raw_tools().len(),
                    "injecting tunnel tools"
                );
                bundle.push(tunnel_tool.clone());
            }
        }
    }

    // ── Run the LLM ↔ tool-call loop ─────────────────────────────────────
    // ModelConfig::to_request maps kind="claude-code" to Provider::ClaudeCode,
    // so every provider can share the same streaming/tool/persistence loop.
    let request = frontier_cfg.to_request().system_prompt(system_prompt);
    let http = reqwest::Client::new();

    // ── In-band compaction pre-phase ─────────────────────────────────────
    // If the previous turn pushed context over the trigger, summarise first —
    // a visible summarise turn through the *same* request and tools (cache-warm,
    // model may still act) — then bridge back to the user's request. See
    // compact.rs for the boundary/reminder mechanics.
    if crate::compact::should_compact(&ctx.pool, ctx.conversation_id, &frontier_cfg).await {
        if let Err(e) = run_compaction_phase(
            ctx,
            &http,
            &request,
            &bundle,
            &frontier_cfg,
            frontier_model_id,
            &mut messages,
        )
        .await
        {
            let msg = format!("{e:#}");
            warn!(conversation = %ctx.conversation_id, "in-band compaction failed: {msg}");
            emit(ctx, json!({"type": "compact_failed", "error": &msg})).await;
            return Err(e);
        }
    }

    generation_loop(
        ctx,
        &http,
        &request,
        messages,
        &bundle,
        frontier_cfg,
        frontier_model_id,
    )
    .await
}

/// In-band compaction: inject a visible summarise trigger, run one generation
/// pass to produce the summary (cache-warm, tools available), finalise the
/// boundary + reminder, then reload the compacted window and append the
/// "continue" bridge so the caller's `generation_loop` resumes the request.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_compaction_phase(
    ctx: &WorkerContext,
    http: &reqwest::Client,
    request: &Request,
    bundle: &ToolBundle,
    model: &ModelConfig,
    model_id: Option<Uuid>,
    messages: &mut Vec<Message>,
) -> anyhow::Result<()> {
    emit(
        ctx,
        json!({
            "type": "compact_started",
            "trigger_tokens": model.compact_trigger_tokens,
        }),
    )
    .await;

    // Phase 1: summarise. The trigger is a normal visible user message; the
    // model's reply (the summary) rides the normal loop and may use tools.
    let trigger = Message::User(vec![UserContent::Text {
        text: crate::compact::summarize_trigger_text(),
    }]);
    let trigger_id = ctx
        .db
        .append(ctx.conversation_id, ctx.user_id, &trigger, None)
        .await
        .map_err(|e| anyhow::anyhow!("persist summarise trigger failed: {e}"))?;
    messages.push(trigger);
    generation_loop(
        ctx,
        http,
        request,
        messages.clone(),
        bundle,
        model.clone(),
        model_id,
    )
    .await?;

    // Phase 1b: attach memory+plan reminder to the summary and set the boundary.
    crate::compact::finalize_compaction(ctx, model, trigger_id).await?;

    // Phase 2 prep: reload the compacted window, then bridge back to the request.
    *messages = sanitize_history(
        crate::compact::load_for_generation(&ctx.db, ctx.conversation_id, ctx.user_id).await?,
    );
    let cont = Message::User(vec![UserContent::Text {
        text: crate::compact::continue_message(ctx).await,
    }]);
    ctx.db
        .append(ctx.conversation_id, ctx.user_id, &cont, None)
        .await
        .map_err(|e| anyhow::anyhow!("persist continue bridge failed: {e}"))?;
    messages.push(cont);
    Ok(())
}

// ── Generation loop ───────────────────────────────────────────────────────────

async fn generation_loop(
    ctx: &WorkerContext,
    http: &reqwest::Client,
    base_request: &Request,
    mut messages: Vec<Message>,
    tools: &ToolBundle,
    compact_model: ModelConfig,
    model_id: Option<Uuid>,
) -> anyhow::Result<()> {
    let tool_defs: Vec<ToolDefinition> = tools.raw_tools();
    let mut ttft_written = false; // only record TTFT for the first LLM call

    loop {
        // ── Check abort / interrupt ───────────────────────────────────────
        if check_stop_reason(&ctx.pool, ctx.job_id).await.is_some() {
            emit(ctx, json!({"type": "aborted"})).await;
            set_job_status(&ctx.pool, ctx.job_id, "aborted", None).await;
            return Ok(());
        }

        // ── Enforce history token budget (no truncation) ──────────────────
        // Safety ceiling above the compact trigger. In-band compaction (run
        // before this loop in run_worker_inner) normally keeps context under
        // the trigger. If it didn't, we refuse the turn with a visible error
        // rather than silently dropping messages — losing context mid-turn is
        // worse than an actionable failure the caller can surface.
        let history_budget = (compact_model.compact_trigger_tokens * 5 / 4) as usize;
        let est_tokens: usize = messages.iter().map(|m| m.estimate_tokens()).sum();
        if est_tokens > history_budget {
            return Err(anyhow::anyhow!(
                "history exceeds token budget ({est_tokens} > {history_budget} tokens); \
                 in-band compaction did not reduce context enough"
            ));
        }

        // ── Open streaming message row in DB ──────────────────────────────
        // This is the single source of truth for partial content.
        // The interrupt handler can seal it; the worker seals it on completion.
        let streaming_msg_id = ctx
            .db
            .append_streaming(ctx.conversation_id, ctx.job_id)
            .await
            .map_err(|e| anyhow::anyhow!("append_streaming failed: {e}"))?;

        // ── Call LLM ──────────────────────────────────────────────────────
        let req = base_request
            .clone()
            .messages(messages.clone())
            .tools(tool_defs.clone());
        let t_llm = std::time::Instant::now();
        let mut stream = match req.stream(http).await {
            Ok(s) => s,
            Err(e) => {
                let _ = ctx
                    .db
                    .seal_streaming_message(streaming_msg_id, None, None, None, None, None)
                    .await;
                return Err(anyhow::anyhow!("LLM stream failed: {e}"));
            }
        };
        info!(ms = t_llm.elapsed().as_millis(), "⏱ LLM stream connected");

        let mut reply_buf = String::new();
        let mut reasoning_buf = String::new();
        let mut tool_calls_buf: Vec<agentix::ToolCall> = Vec::new();
        let mut provider_state: Option<serde_json::Value> = None;
        let mut usage = UsageStats::default();
        let mut token_count: u32 = 0;
        let mut ttft_logged = false;
        let mut ttfa_logged = false; // first event of any kind (content, reasoning, tool call)
        // Per-request latency captured here so each token_usage_events row
        // can be joined back to TTFT / total wall-time without scraping logs.
        let mut ttft_ms: Option<i64> = None;

        // ── Consume stream ────────────────────────────────────────────────
        loop {
            if let Some(_reason) = check_stop_reason(&ctx.pool, ctx.job_id).await {
                // Seal the streaming row with whatever we have — the interrupt
                // handler may also call seal (idempotent).
                let _ = ctx
                    .db
                    .seal_streaming_message(
                        streaming_msg_id,
                        if reply_buf.is_empty() {
                            None
                        } else {
                            Some(&reply_buf)
                        },
                        if reasoning_buf.is_empty() {
                            None
                        } else {
                            Some(&reasoning_buf)
                        },
                        None,
                        crate::db::MessageContext::from_usage(&usage),
                        provider_state.as_ref(),
                    )
                    .await;
                emit(ctx, json!({"type": "aborted"})).await;
                set_job_status(&ctx.pool, ctx.job_id, "aborted", None).await;
                return Ok(());
            }

            let event = stream.next().await;
            if !ttfa_logged {
                if let Some(ev) = &event {
                    if matches!(
                        ev,
                        LlmEvent::Token(_)
                            | LlmEvent::Reasoning(_)
                            | LlmEvent::ToolCallChunk(_)
                            | LlmEvent::ToolCall(_)
                    ) {
                        ttfa_logged = true;
                        info!(
                            ms = t_llm.elapsed().as_millis(),
                            "⏱ TTFA (first event of any kind)"
                        );
                    }
                }
            }
            match event {
                None | Some(LlmEvent::Done) => break,

                Some(LlmEvent::Token(token)) => {
                    if !ttft_logged {
                        ttft_logged = true;
                        let ttft = t_llm.elapsed().as_millis() as i64;
                        ttft_ms = Some(ttft);
                        info!(ms = ttft, "⏱ TTFT (first token)");
                        if !ttft_written {
                            ttft_written = true;
                            record_job_latency(&ctx.pool, ctx.job_id, Some(ttft), None, None, None)
                                .await;
                        }
                    }
                    reply_buf.push_str(&token);
                    emit(ctx, json!({"type": "token", "content": token})).await;
                    // Batch DB update every 10 tokens to reduce MVCC overhead.
                    token_count += 1;
                    if token_count.is_multiple_of(10) {
                        let _ = ctx
                            .db
                            .update_streaming_content(streaming_msg_id, &reply_buf, &reasoning_buf)
                            .await;
                    }
                }

                Some(LlmEvent::Reasoning(token)) => {
                    reasoning_buf.push_str(&token);
                    emit(ctx, json!({"type": "reasoning_token", "content": token})).await;
                }

                Some(LlmEvent::ToolCallChunk(c)) => {
                    emit(
                        ctx,
                        json!({"type": "tool_call", "id": c.id, "name": c.name, "delta": c.delta}),
                    )
                    .await;
                }

                Some(LlmEvent::ToolCall(tc)) => {
                    emit(
                        ctx,
                        json!({"type": "tool_call_complete", "id": tc.id, "name": tc.name}),
                    )
                    .await;
                    tool_calls_buf.push(tc);
                }

                Some(LlmEvent::Usage(u)) => {
                    usage += u.clone();
                    // Usage events typically arrive at the tail of the stream
                    // (Anthropic: in message_delta; OpenAI: after the last
                    // content chunk). `t_llm.elapsed()` here is a faithful
                    // proxy for total request wall-time.
                    let total_ms = t_llm.elapsed().as_millis() as i64;
                    persist_usage_event(
                        ctx,
                        streaming_msg_id,
                        model_id,
                        &u,
                        ttft_ms,
                        Some(total_ms),
                    )
                    .await;
                }

                Some(LlmEvent::AssistantState(v)) => {
                    provider_state = Some(v);
                }

                Some(LlmEvent::Error(err_msg)) => {
                    error!(conversation = %ctx.conversation_id, "stream error: {err_msg}");
                    let is_benign =
                        err_msg.contains("Error in input stream") && !reply_buf.trim().is_empty();
                    if is_benign {
                        warn!(conversation = %ctx.conversation_id, "treating benign tail error as done");
                        break;
                    }
                    let _ = ctx
                        .db
                        .seal_streaming_message(streaming_msg_id, None, None, None, None, None)
                        .await;
                    return Err(anyhow::anyhow!("{err_msg}"));
                }

                Some(_) => {}
            }
        }

        // ── Emit usage summary ────────────────────────────────────────────
        if usage.total_tokens > 0 {
            let context_tokens = usage.prompt_tokens as i64
                + usage.cache_read_tokens as i64
                + usage.cache_creation_tokens as i64;
            emit(
                ctx,
                json!({
                    "type": "usage",
                    "prompt_tokens": usage.prompt_tokens,
                    "completion_tokens": usage.completion_tokens,
                    "total_tokens": usage.total_tokens,
                    "context_tokens": context_tokens,
                    "compact_trigger_tokens": compact_model.compact_trigger_tokens,
                }),
            )
            .await;
        }

        // ── Seal the streaming message with final content ─────────────────
        // Drop any tool calls whose arguments are not a complete, valid JSON object.
        // This guards against truncated streams where agentix's finalize() emits
        // a PartialToolCall with incomplete JSON (e.g. `{"writes": `), which would
        // cause Anthropic to reject subsequent requests with HTTP 400.
        tool_calls_buf.retain(|tc| {
            serde_json::from_str::<serde_json::Value>(&tc.arguments)
                .map(|v| v.is_object())
                .unwrap_or(false)
        });
        let tc_json = if tool_calls_buf.is_empty() {
            None
        } else {
            serde_json::to_string(&tool_calls_buf).ok()
        };
        let _ = ctx
            .db
            .seal_streaming_message(
                streaming_msg_id,
                if reply_buf.is_empty() {
                    None
                } else {
                    Some(&reply_buf)
                },
                if reasoning_buf.is_empty() {
                    None
                } else {
                    Some(&reasoning_buf)
                },
                tc_json.as_deref(),
                crate::db::MessageContext::from_usage(&usage),
                provider_state.as_ref(),
            )
            .await;

        // Kick off embedding for the sealed text (fire-and-forget).
        if !reply_buf.is_empty() {
            embed_message_async(ctx, streaming_msg_id, reply_buf.clone());
        }

        let assistant_msg = Message::Assistant {
            content: if reply_buf.is_empty() {
                None
            } else {
                Some(reply_buf.clone())
            },
            reasoning: if reasoning_buf.is_empty() {
                None
            } else {
                Some(reasoning_buf)
            },
            tool_calls: tool_calls_buf.clone(),
            provider_data: provider_state.clone(),
        };
        if !reply_buf.is_empty() || !tool_calls_buf.is_empty() {
            messages.push(assistant_msg);
        }

        // ── No tool calls → done ─────────────────────────────────────────
        if tool_calls_buf.is_empty() {
            break;
        }

        // ── Execute tools ─────────────────────────────────────────────────
        for tc in &tool_calls_buf {
            let args: Value = serde_json::from_str(&tc.arguments).unwrap_or(json!({}));

            emit(
                ctx,
                json!({"type": "tool_progress", "id": tc.id, "name": tc.name, "progress": "executing..."}),
            )
            .await;

            let mut tool_stream = tools.call(&tc.name, args).await;
            let mut result_val: Vec<agentix::Content> = Vec::new();
            let mut aborted_during_tool = false;

            // Poll for abort every 500 ms concurrently with the tool stream.
            // Using a separate async block ensures the abort check races the
            // tool future properly even when tool_stream.next() never resolves.
            let abort_poll = async {
                loop {
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    if check_stop_reason(&ctx.pool, ctx.job_id).await.is_some() {
                        return;
                    }
                }
            };
            tokio::pin!(abort_poll);

            loop {
                tokio::select! {
                    biased;
                    _ = &mut abort_poll => {
                        aborted_during_tool = true;
                        break;
                    }
                    output = tool_stream.next() => {
                        match output {
                            None => break,
                            Some(ToolOutput::Progress(p)) => {
                                let parsed = serde_json::from_str::<Value>(&p).ok();
                                let is_spawn = parsed.as_ref()
                                    .and_then(|v| v.get("source"))
                                    .and_then(|s| s.as_str()) == Some("spawn");
                                if is_spawn {
                                    emit(ctx, parsed.unwrap()).await;
                                } else {
                                    emit(
                                        ctx,
                                        json!({"type": "tool_progress", "id": tc.id, "name": tc.name, "progress": p}),
                                    )
                                    .await;
                                }
                            }
                            Some(ToolOutput::Result(v)) => {
                                result_val = v;
                            }
                        }
                    }
                }
            }

            if aborted_during_tool {
                emit(ctx, json!({"type": "aborted"})).await;
                set_job_status(&ctx.pool, ctx.job_id, "aborted", None).await;
                return Ok(());
            }

            // Persist and emit tool result
            let tool_result_msg = Message::ToolResult {
                call_id: tc.id.clone(),
                content: result_val.clone(),
            };
            persist_msg(ctx, &tool_result_msg).await;
            messages.push(tool_result_msg);

            // For SSE emit and __ask__ check, get the text content as a Value
            let result_json: Value = result_val
                .iter()
                .find_map(|p| {
                    if let agentix::Content::Text { text } = p {
                        serde_json::from_str(text).ok()
                    } else {
                        None
                    }
                })
                .unwrap_or(Value::Null);

            // Collect image content — resolve __sandbox__: refs to base64 data URIs
            // so the browser can display them directly.
            let mut sse_images: Vec<Value> = Vec::new();
            for part in &result_val {
                if let agentix::Content::Image(img) = part {
                    use base64::Engine as _;
                    let data_uri = match &img.data {
                        agentix::request::ImageData::Url(u) if u.starts_with("__sandbox__:") => {
                            let filename = &u["__sandbox__:".len()..];
                            let file_path = ctx
                                .sandbox
                                .get_conversation_dir(ctx.user_id, ctx.conversation_id)
                                .join(filename);
                            match tokio::fs::read(&file_path).await {
                                Ok(bytes) => Some(format!(
                                    "data:{};base64,{}",
                                    img.mime_type,
                                    base64::engine::general_purpose::STANDARD.encode(&bytes)
                                )),
                                Err(_) => None,
                            }
                        }
                        agentix::request::ImageData::Url(u) => Some(u.clone()),
                        agentix::request::ImageData::Base64(b) => {
                            Some(format!("data:{};base64,{}", img.mime_type, b))
                        }
                    };
                    if let Some(uri) = data_uri {
                        sse_images.push(json!({"url": uri, "mime_type": img.mime_type}));
                    }
                }
            }

            let mut emit_payload =
                json!({"type": "tool_result", "id": tc.id, "name": tc.name, "result": result_json});
            if !sse_images.is_empty() {
                emit_payload["images"] = Value::Array(sse_images);
            }
            emit(ctx, emit_payload).await;

            // If the tool returned an __ask__ marker, emit an ask event and end generation.
            // The user's answer will arrive as a new message, starting a new generation job.
            // We return Ok(()) here — the outer run_worker will emit {"type":"done"} and
            // set the job status to "done" uniformly.
            if result_json.get("__ask__").and_then(|v| v.as_bool()) == Some(true) {
                emit(
                    ctx,
                    json!({
                        "type": "ask",
                        "question": result_json.get("question").and_then(|v| v.as_str()).unwrap_or(""),
                        "description": result_json.get("description"),
                        "options": result_json.get("options"),
                    }),
                )
                .await;
                return Ok(());
            }
        }

        // Loop back → send tool results to LLM
    }

    Ok(())
}

async fn persist_usage_event(
    ctx: &WorkerContext,
    message_id: i64,
    model_id: Option<Uuid>,
    usage: &UsageStats,
    ttft_ms: Option<i64>,
    total_ms: Option<i64>,
) {
    // Compact only needs the model's input-context size on this turn:
    // non-cached prompt + cache read + cache creation.
    let context_tokens = usage.prompt_tokens as i64
        + usage.cache_read_tokens as i64
        + usage.cache_creation_tokens as i64;
    let now = chrono::Utc::now().timestamp();

    let _ = sqlx::query(
        r#"INSERT INTO token_usage_events
               (job_id, conversation_id, user_id, message_id, conversation_name,
                prompt_tokens, completion_tokens, cache_read_tokens,
                cache_creation_tokens, model_id, ttft_ms, total_ms, created_at)
           SELECT $1, c.id, c.user_id, $2, c.name,
                  $3, $4, $5, $6, $7, $8, $9, $10
           FROM conversations c
           WHERE c.id = $11"#,
    )
    .bind(ctx.job_id)
    .bind(message_id)
    .bind(usage.prompt_tokens as i64)
    .bind(usage.completion_tokens as i64)
    .bind(usage.cache_read_tokens as i64)
    .bind(usage.cache_creation_tokens as i64)
    .bind(model_id)
    .bind(ttft_ms)
    .bind(total_ms)
    .bind(now)
    .bind(ctx.conversation_id)
    .execute(&ctx.pool)
    .await;

    let _ = sqlx::query("UPDATE messages SET context_tokens = $2 WHERE id = $1")
        .bind(message_id)
        .bind(context_tokens)
        .execute(&ctx.pool)
        .await;
}

// ── MCP loading from DB ───────────────────────────────────────────────────────

async fn connect_mcps_from_db(ctx: &WorkerContext) -> Vec<(String, McpTool)> {
    // Load global MCPs
    let global_cfg = Config::load_from_db(&ctx.pool).await.unwrap_or_default();
    let mut all_tools: Vec<(String, McpTool)> = Vec::new();

    for mc in &global_cfg.mcp {
        match mc {
            crate::config::McpServerConfig::Studio {
                name,
                command,
                args,
                env,
            } => {
                let env_args: Vec<String>;
                let (cmd, final_args): (&str, Vec<&str>) = if env.is_empty() {
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
                match McpTool::stdio(cmd, &final_args).await {
                    Ok(t) => {
                        info!("MCP: {name} ready ({} tools)", t.raw_tools().len());
                        all_tools.push((name.clone(), t));
                    }
                    Err(e) => warn!("MCP: {name} failed to start: {e}"),
                }
            }
            crate::config::McpServerConfig::Http { name, url } => match McpTool::http(url).await {
                Ok(t) => {
                    info!("MCP: {name} ready ({} tools)", t.raw_tools().len());
                    all_tools.push((name.clone(), t));
                }
                Err(e) => warn!("MCP: {name} failed to start: {e}"),
            },
        }
    }

    // Load user + conversation MCPs
    let mcp_rows: Vec<(String, String, Value)> = sqlx::query_as(
        r#"SELECT name, "type", config FROM user_mcps WHERE user_id = $1
           UNION
           SELECT name, "type", config FROM conversation_mcps WHERE conversation_id = $2
           ORDER BY name ASC"#,
    )
    .bind(ctx.user_id)
    .bind(ctx.conversation_id)
    .fetch_all(&ctx.pool)
    .await
    .unwrap_or_default();

    for (name, mcp_type, config) in mcp_rows {
        let tool = match mcp_type.as_str() {
            "http" => {
                let url = config
                    .get("url")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                match timeout(Duration::from_secs(15), McpTool::http(&url)).await {
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
                let (cmd, args_wrapped) = ctx.sandbox.wrap_mcp_command(
                    ctx.user_id,
                    ctx.conversation_id,
                    &command,
                    &args_ref,
                );
                let args_wrapped_ref: Vec<&str> = args_wrapped.iter().map(String::as_str).collect();
                match timeout(
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
        all_tools.push((name, tool));
    }

    all_tools
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Kick off embedding computation for an already-persisted message row (fire-and-forget).
fn embed_message_async(ctx: &WorkerContext, row_id: i64, content: String) {
    let pool = ctx.pool.clone();
    let db = ctx.db.clone();
    tokio::spawn(async move {
        let global_cfg = Config::load_from_db(&pool).await.unwrap_or_default();
        let embed_client = EmbeddingClient::new(
            global_cfg.embedding.api_key,
            global_cfg.embedding.api_base,
            global_cfg.embedding.name,
        );
        match embed_client.embed(&content).await {
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

/// Write an SSE event.
///
/// Token events skip the DB INSERT and go directly via pg_notify for low latency.
/// Structural events (tool_call, done, error, aborted, usage, …) are persisted to
/// generation_events for reliable replay, then notified.
pub(crate) async fn emit(ctx: &WorkerContext, payload: Value) {
    let payload_str = payload.to_string();
    let event_type = payload.get("type").and_then(|v| v.as_str()).unwrap_or("");

    // Fast path: token / reasoning_token — skip DB, notify with inline payload.
    // Prefix "I:" signals an inline payload to the SSE consumer.
    if matches!(event_type, "token" | "reasoning_token") {
        let notify_payload = format!("I:{}:{}", ctx.job_id, payload_str);
        // pg_notify payload limit is 8000 bytes; tokens are tiny, this is safe.
        if notify_payload.len() < 7900 {
            let _ = sqlx::query("SELECT pg_notify('generation_events', $1)")
                .bind(&notify_payload)
                .execute(&ctx.pool)
                .await;
            return;
        }
        // Fallback: payload too large, persist normally.
    }

    // Reliable path: persist to DB (trigger fires pg_notify with "job_id:event_id").
    if let Err(e) = sqlx::query("INSERT INTO generation_events (job_id, payload) VALUES ($1, $2)")
        .bind(ctx.job_id)
        .bind(&payload_str)
        .execute(&ctx.pool)
        .await
    {
        error!(job = %ctx.job_id, "failed to emit event: {e}");
    }
}

/// Reason the worker should stop early.
#[derive(Debug, Clone, Copy, PartialEq)]
enum StopReason {
    /// Normal abort: worker must persist any partial reply itself.
    Aborted,
    /// Interrupt handler already saved the partial — worker just exits cleanly.
    Interrupted,
}

/// Returns `Some(reason)` if the job should stop, `None` to keep going.
async fn check_stop_reason(pool: &PgPool, job_id: Uuid) -> Option<StopReason> {
    let status: Option<String> =
        sqlx::query_scalar("SELECT status FROM generation_jobs WHERE id = $1")
            .bind(job_id)
            .fetch_optional(pool)
            .await
            .unwrap_or(None);
    match status.as_deref() {
        Some("aborted") => Some(StopReason::Aborted),
        Some("interrupted") => Some(StopReason::Interrupted),
        _ => None,
    }
}

async fn record_job_latency(
    pool: &PgPool,
    job_id: Uuid,
    ttft_ms: Option<i64>,
    duration_ms: Option<i64>,
    model: Option<&str>,
    provider: Option<&str>,
) {
    let _ = sqlx::query(
        "UPDATE generation_jobs
         SET ttft_ms     = COALESCE($1, ttft_ms),
             duration_ms = COALESCE($2, duration_ms),
             model       = COALESCE($3, model),
             provider    = COALESCE($4, provider),
             updated_at  = now()
         WHERE id = $5",
    )
    .bind(ttft_ms)
    .bind(duration_ms)
    .bind(model)
    .bind(provider)
    .bind(job_id)
    .execute(pool)
    .await;
}

async fn set_job_status(pool: &PgPool, job_id: Uuid, status: &str, error: Option<&str>) {
    let _ = sqlx::query(
        "UPDATE generation_jobs SET status = $1, error = $2, updated_at = now() WHERE id = $3",
    )
    .bind(status)
    .bind(error)
    .bind(job_id)
    .execute(pool)
    .await;
}

/// Persist a message to the messages table with embedding (async).
async fn persist_msg(ctx: &WorkerContext, msg: &Message) {
    let db = ctx.db.clone();
    let conv_id = ctx.conversation_id;

    let text_for_embed: Option<String> = match msg {
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

    let row_id = match db.append(conv_id, ctx.user_id, msg, None).await {
        Ok(id) => id,
        Err(e) => {
            error!("db append failed: {e}");
            return;
        }
    };

    if let Some(content) = text_for_embed {
        let pool = ctx.pool.clone();
        let global_cfg = Config::load_from_db(&pool).await.unwrap_or_default();
        let embed_client = EmbeddingClient::new(
            global_cfg.embedding.api_key,
            global_cfg.embedding.api_base,
            global_cfg.embedding.name,
        );
        tokio::spawn(async move {
            match embed_client.embed(&content).await {
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

// ── History sanitization (moved from state.rs) ────────────────────────────────

/// Remove tool_calls from assistant messages that have no matching ToolResult,
/// and remove ToolResult messages that have no matching tool_call in history.
pub(crate) fn sanitize_history(messages: Vec<Message>) -> Vec<Message> {
    use std::collections::HashSet;

    let answered: HashSet<&str> = messages
        .iter()
        .filter_map(|m| {
            if let Message::ToolResult { call_id, .. } = m {
                Some(call_id.as_str())
            } else {
                None
            }
        })
        .collect();

    // Collect all tool_call ids that appear in assistant messages.
    let called: HashSet<&str> = messages
        .iter()
        .filter_map(|m| {
            if let Message::Assistant { tool_calls, .. } = m {
                Some(tool_calls.iter().map(|tc| tc.id.as_str()))
            } else {
                None
            }
        })
        .flatten()
        .collect();

    let mut result: Vec<Message> = Vec::with_capacity(messages.len());
    for msg in &messages {
        match msg {
            Message::Assistant {
                content,
                reasoning,
                tool_calls,
                ..
            } if !tool_calls.is_empty() => {
                let kept: Vec<_> = tool_calls
                    .iter()
                    .filter(|tc| answered.contains(tc.id.as_str()))
                    .cloned()
                    .collect();
                if !kept.is_empty() {
                    result.push(Message::Assistant {
                        content: content.clone(),
                        reasoning: reasoning.clone(),
                        tool_calls: kept,
                        // Filtering drops some tool_calls; any provider_data
                        // would reference removed tool_use blocks by ID.
                        provider_data: None,
                    });
                } else if content.as_deref().map(|s| !s.is_empty()).unwrap_or(false) {
                    result.push(Message::Assistant {
                        content: content.clone(),
                        reasoning: reasoning.clone(),
                        tool_calls: vec![],
                        provider_data: None,
                    });
                }
            }
            // Drop orphaned tool results (no matching tool_call in history).
            Message::ToolResult { call_id, .. } if !called.contains(call_id.as_str()) => {}
            other => result.push(other.clone()),
        }
    }
    result
}
