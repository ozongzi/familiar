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

use agentix::{
    LlmEvent, McpTool, Message, Request, ToolBundle, Tool, ToolOutput, UserContent,
};
use agentix::raw::shared::ToolDefinition;
use agentix::types::UsageStats;
use futures::StreamExt;
use serde_json::{json, Value};
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

    let result = run_worker_inner(&ctx).await;

    match &result {
        Ok(()) => {
            emit(&ctx, json!({"type": "done"})).await;
            set_job_status(&ctx.pool, ctx.job_id, "done", None).await;
        }
        Err(e) => {
            let msg = e.to_string();
            emit(&ctx, json!({"type": "error", "message": &msg})).await;
            set_job_status(&ctx.pool, ctx.job_id, "error", Some(&msg)).await;
        }
    }

    result
}

async fn run_worker_inner(ctx: &WorkerContext) -> anyhow::Result<()> {
    let t0 = std::time::Instant::now();
    let global_cfg = Config::load_from_db(&ctx.pool).await.unwrap_or_default();
    info!(ms = t0.elapsed().as_millis(), "⏱ load_from_db");

    // ── Resolve cheap model + system prompt from user settings ────────────
    let user_settings: Option<(Option<Value>, Option<String>)> = sqlx::query_as(
        "SELECT cheap_model, system_prompt FROM user_settings WHERE user_id = $1",
    )
    .bind(ctx.user_id)
    .fetch_optional(&ctx.pool)
    .await
    .unwrap_or(None);

    let (cheap_cfg, mut system_prompt) = if let Some((c, p)) = user_settings {
        let c_cfg: ModelConfig = c
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_else(|| global_cfg.cheap_model.clone());
        let s_prompt = p.or_else(|| global_cfg.system_prompt());
        (c_cfg, s_prompt)
    } else {
        (global_cfg.cheap_model.clone(), global_cfg.system_prompt())
    };

    // ── Resolve frontier model: conversation model_id > global default ────
    let frontier_cfg: ModelConfig = {
        fn model_from_row(provider: String, name: String, api_base: String, api_key: String, extra_body: Value) -> ModelConfig {
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
            }
        }

        // 1. conversation-level model_id
        let conv_model: Option<(String, String, String, String, Value)> = sqlx::query_as(
            "SELECT m.provider, m.model_name, m.api_base, m.api_key, m.extra_body
             FROM conversations c
             JOIN models m ON m.id = c.model_id
             WHERE c.id = $1",
        )
        .bind(ctx.conversation_id)
        .fetch_optional(&ctx.pool)
        .await
        .unwrap_or(None);

        if let Some((provider, name, api_base, api_key, extra_body)) = conv_model {
            model_from_row(provider, name, api_base, api_key, extra_body)
        } else {
            // 2. global default model
            let default_model: Option<(String, String, String, String, Value)> = sqlx::query_as(
                "SELECT provider, model_name, api_base, api_key, extra_body
                 FROM models WHERE scope = 'global' AND is_default = true LIMIT 1",
            )
            .fetch_optional(&ctx.pool)
            .await
            .unwrap_or(None);

            if let Some((provider, name, api_base, api_key, extra_body)) = default_model {
                model_from_row(provider, name, api_base, api_key, extra_body)
            } else {
                // 3. fallback: cheap_model
                cheap_cfg.clone()
            }
        }
    };

    // ── Resolve user name ─────────────────────────────────────────────────
    let user_name: String =
        sqlx::query_scalar::<_, String>("SELECT name FROM users WHERE id = $1")
            .bind(ctx.user_id)
            .fetch_optional(&ctx.pool)
            .await
            .unwrap_or(None)
            .unwrap_or_default();

    // ── Append skills to system prompt ────────────────────────────────────
    let app_skill_rows: Vec<(String, Option<String>)> =
        sqlx::query_as("SELECT name, description FROM app_skills ORDER BY name ASC")
            .fetch_all(&ctx.pool)
            .await
            .unwrap_or_default();
    let user_skill_rows: Vec<(String, Option<String>)> = sqlx::query_as(
        "SELECT name, description FROM user_skills WHERE user_id = $1 ORDER BY name ASC",
    )
    .bind(ctx.user_id)
    .fetch_all(&ctx.pool)
    .await
    .unwrap_or_default();

    let mut skills: Vec<String> = app_skill_rows
        .into_iter()
        .chain(user_skill_rows)
        .map(|(name, desc)| match desc {
            Some(d) => format!("- {name}: {d}"),
            None => format!("- {name}"),
        })
        .collect();

    if !skills.is_empty() {
        skills.sort();
        skills.dedup();
        let summary = format!(
            "\n\n可用 Skills（需要时调用 load_skill 获取详细指令）：\n{}",
            skills.join("\n")
        );
        system_prompt = Some(system_prompt.unwrap_or_default() + &summary);
    }

    // ── Append plan to system prompt ──────────────────────────────────────
    let plan_row: Option<(String, String)> = sqlx::query_as(
        "SELECT title, steps_json FROM conversation_plans WHERE conversation_id = $1",
    )
    .bind(ctx.conversation_id)
    .fetch_optional(&ctx.pool)
    .await
    .unwrap_or(None);

    if let Some((plan_title, plan_steps)) = plan_row {
        let plan_section = format!(
            "\n\n## 当前执行计划\n标题：{}\n步骤（JSON）：{}\n\n每次更新步骤状态时，调用 todo_list 工具同步最新进度。",
            plan_title, plan_steps
        );
        system_prompt = Some(system_prompt.unwrap_or_default() + &plan_section);
    }

    // ── Append memories to system prompt ─────────────────────────────────
    if let Some(mem_section) = crate::spells::load_memories_for_prompt(&ctx.pool, ctx.user_id).await {
        system_prompt = Some(system_prompt.unwrap_or_default() + &mem_section);
    }

    // ── Build Request ─────────────────────────────────────────────────────
    let mut request = frontier_cfg.to_request();
    if let Some(prompt) = &system_prompt {
        let rendered = crate::prompt_template::render_prompt(
            prompt,
            &[("USER_NAME", &user_name)],
        );
        request = request.system_prompt(rendered);
    }

    // ── Load history from DB ──────────────────────────────────────────────
    let t_restore = std::time::Instant::now();
    let messages = match ctx.db.restore(ctx.conversation_id).await {
        Ok(h) => {
            let h = sanitize_history(h);
            info!(conversation = %ctx.conversation_id, messages = h.len(), ms = t_restore.elapsed().as_millis(), "⏱ restore history");
            h
        }
        Err(e) => {
            error!(conversation = %ctx.conversation_id, "failed to restore history: {e}");
            vec![]
        }
    };

    // ── Connect MCPs from DB ──────────────────────────────────────────────
    let t_mcp = std::time::Instant::now();
    let mcp_tools = connect_mcps_from_db(ctx).await;
    info!(ms = t_mcp.elapsed().as_millis(), tools = mcp_tools.len(), "⏱ connect_mcps");
    info!(ms = t0.elapsed().as_millis(), "⏱ total pre-LLM setup");

    // ── Build ToolBundle (spells + MCPs + tunnel) ─────────────────────────
    let spell_deps = SpellDeps {
        subagent_prompt: global_cfg.subagent_prompt(),
        cheap_model: cheap_cfg,
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
    let http = reqwest::Client::new();
    generation_loop(ctx, &http, &request, messages, &bundle).await
}

// ── Generation loop ───────────────────────────────────────────────────────────

const HISTORY_TOKEN_BUDGET: usize = 25_000;

async fn generation_loop(
    ctx: &WorkerContext,
    http: &reqwest::Client,
    base_request: &Request,
    mut messages: Vec<Message>,
    tools: &ToolBundle,
) -> anyhow::Result<()> {
    let tool_defs: Vec<ToolDefinition> = tools.raw_tools();

    loop {
        // ── Check abort / interrupt ───────────────────────────────────────
        if check_stop_reason(&ctx.pool, ctx.job_id).await.is_some() {
            emit(ctx, json!({"type": "aborted"})).await;
            set_job_status(&ctx.pool, ctx.job_id, "aborted", None).await;
            return Ok(());
        }

        // ── Truncate history to token budget ──────────────────────────────
        let before = messages.len();
        agentix::truncate_to_token_budget(&mut messages, HISTORY_TOKEN_BUDGET);
        if messages.len() < before {
            info!(conversation = %ctx.conversation_id, dropped = before - messages.len(), kept = messages.len(), "history truncated");
        }

        // ── Open streaming message row in DB ──────────────────────────────
        // This is the single source of truth for partial content.
        // The interrupt handler can seal it; the worker seals it on completion.
        let streaming_msg_id = ctx.db
            .append_streaming(ctx.conversation_id, ctx.job_id)
            .await
            .map_err(|e| anyhow::anyhow!("append_streaming failed: {e}"))?;

        // ── Call LLM ──────────────────────────────────────────────────────
        let req = base_request.clone()
            .messages(messages.clone())
            .tools(tool_defs.clone());
        let mut stream = match req.stream(http).await {
            Ok(s) => s,
            Err(e) => {
                let _ = ctx.db.seal_streaming_message(streaming_msg_id, None, None, None).await;
                return Err(anyhow::anyhow!("LLM stream failed: {e}"));
            }
        };

        let mut reply_buf = String::new();
        let mut reasoning_buf = String::new();
        let mut tool_calls_buf: Vec<agentix::ToolCall> = Vec::new();
        let mut usage = UsageStats::default();
        let mut token_count: u32 = 0;

        // ── Consume stream ────────────────────────────────────────────────
        loop {
            if let Some(_reason) = check_stop_reason(&ctx.pool, ctx.job_id).await {
                // Seal the streaming row with whatever we have — the interrupt
                // handler may also call seal (idempotent).
                let _ = ctx.db.seal_streaming_message(
                    streaming_msg_id,
                    if reply_buf.is_empty() { None } else { Some(&reply_buf) },
                    if reasoning_buf.is_empty() { None } else { Some(&reasoning_buf) },
                    None,
                ).await;
                emit(ctx, json!({"type": "aborted"})).await;
                set_job_status(&ctx.pool, ctx.job_id, "aborted", None).await;
                return Ok(());
            }

            let event = stream.next().await;
            match event {
                None | Some(LlmEvent::Done) => break,

                Some(LlmEvent::Token(token)) => {
                    reply_buf.push_str(&token);
                    emit(ctx, json!({"type": "token", "content": token})).await;
                    // Batch DB update every 10 tokens to reduce MVCC overhead.
                    token_count += 1;
                    if token_count % 10 == 0 {
                        let _ = ctx.db.update_streaming_content(
                            streaming_msg_id, &reply_buf, &reasoning_buf,
                        ).await;
                    }
                }

                Some(LlmEvent::Reasoning(token)) => {
                    reasoning_buf.push_str(&token);
                    emit(ctx, json!({"type": "reasoning_token", "content": token})).await;
                }

                Some(LlmEvent::ToolCallChunk(c)) => {
                    emit(ctx, json!({"type": "tool_call", "id": c.id, "name": c.name, "delta": c.delta})).await;
                }

                Some(LlmEvent::ToolCall(tc)) => {
                    emit(ctx, json!({"type": "tool_call_complete", "id": tc.id, "name": tc.name})).await;
                    tool_calls_buf.push(tc);
                }

                Some(LlmEvent::Usage(u)) => {
                    usage += u.clone();
                    let pool = ctx.pool.clone();
                    let conv_id = ctx.conversation_id;
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
                        .bind(u.prompt_tokens as i64)
                        .bind(u.completion_tokens as i64)
                        .bind(u.total_tokens as i64)
                        .bind(conv_id)
                        .execute(&pool)
                        .await;
                    });
                }

                Some(LlmEvent::Error(err_msg)) => {
                    error!(conversation = %ctx.conversation_id, "stream error: {err_msg}");
                    let is_benign =
                        err_msg.contains("Error in input stream") && !reply_buf.trim().is_empty();
                    if is_benign {
                        warn!(conversation = %ctx.conversation_id, "treating benign tail error as done");
                        break;
                    }
                    let _ = ctx.db.seal_streaming_message(streaming_msg_id, None, None, None).await;
                    return Err(anyhow::anyhow!("{err_msg}"));
                }
            }
        }

        // ── Emit usage summary ────────────────────────────────────────────
        if usage.total_tokens > 0 {
            emit(
                ctx,
                json!({
                    "type": "usage",
                    "prompt_tokens": usage.prompt_tokens,
                    "completion_tokens": usage.completion_tokens,
                    "total_tokens": usage.total_tokens,
                }),
            )
            .await;
        }

        // ── Seal the streaming message with final content ─────────────────
        let tc_json = if tool_calls_buf.is_empty() {
            None
        } else {
            serde_json::to_string(&tool_calls_buf).ok()
        };
        let _ = ctx.db.seal_streaming_message(
            streaming_msg_id,
            if reply_buf.is_empty() { None } else { Some(&reply_buf) },
            if reasoning_buf.is_empty() { None } else { Some(&reasoning_buf) },
            tc_json.as_deref(),
        ).await;

        // Kick off embedding for the sealed text (fire-and-forget).
        if !reply_buf.is_empty() {
            embed_message_async(ctx, streaming_msg_id, reply_buf.clone());
        }

        let assistant_msg = Message::Assistant {
            content: if reply_buf.is_empty() { None } else { Some(reply_buf.clone()) },
            reasoning: if reasoning_buf.is_empty() { None } else { Some(reasoning_buf) },
            tool_calls: tool_calls_buf.clone(),
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
            let mut result_val = json!(null);
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
                content: result_val.to_string(),
            };
            persist_msg(ctx, &tool_result_msg).await;
            messages.push(tool_result_msg);

            emit(
                ctx,
                json!({"type": "tool_result", "id": tc.id, "name": tc.name, "result": result_val}),
            )
            .await;

            // If the tool returned an __ask__ marker, emit an ask event and end generation.
            // The user's answer will arrive as a new message, starting a new generation job.
            // We return Ok(()) here — the outer run_worker will emit {"type":"done"} and
            // set the job status to "done" uniformly.
            if result_val.get("__ask__").and_then(|v| v.as_bool()) == Some(true) {
                emit(
                    ctx,
                    json!({
                        "type": "ask",
                        "question": result_val.get("question").and_then(|v| v.as_str()).unwrap_or(""),
                        "description": result_val.get("description"),
                        "options": result_val.get("options"),
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
            crate::config::McpServerConfig::Http { name, url } => {
                match McpTool::http(url).await {
                    Ok(t) => {
                        info!("MCP: {name} ready ({} tools)", t.raw_tools().len());
                        all_tools.push((name.clone(), t));
                    }
                    Err(e) => warn!("MCP: {name} failed to start: {e}"),
                }
            }
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
                match timeout(Duration::from_secs(300), McpTool::stdio(&cmd, &args_wrapped_ref))
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
async fn emit(ctx: &WorkerContext, payload: Value) {
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
    if let Err(e) = sqlx::query(
        "INSERT INTO generation_events (job_id, payload) VALUES ($1, $2)",
    )
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
        Some("aborted")     => Some(StopReason::Aborted),
        Some("interrupted") => Some(StopReason::Interrupted),
        _                   => None,
    }
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

    let row_id = match db.append(conv_id, msg, None).await {
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

/// Remove tool_calls from assistant messages that have no matching ToolResult.
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

    let mut result: Vec<Message> = Vec::with_capacity(messages.len());
    for msg in &messages {
        match msg {
            Message::Assistant {
                content,
                reasoning,
                tool_calls,
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
                    });
                } else if content.as_deref().map(|s| !s.is_empty()).unwrap_or(false) {
                    result.push(Message::Assistant {
                        content: content.clone(),
                        reasoning: reasoning.clone(),
                        tool_calls: vec![],
                    });
                }
            }
            other => result.push(other.clone()),
        }
    }
    result
}
