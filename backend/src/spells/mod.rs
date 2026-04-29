mod admin_spells;
mod ast_spell;
mod generate_image_spell;
mod history_spell;
mod manage_mcp_spell;
mod memory_spell;
mod plan_spell;
mod sandbox_spell;
mod skill_spell;
mod sourcegraph_spell;
mod spawn_spell;
mod tavily_spell;
mod ui_spells;

use admin_spells::AdminSpells;
use ast_spell::AstSpell;
use generate_image_spell::GenerateImageSpell;
use history_spell::HistorySpell;
use memory_spell::MemorySpell;
pub use memory_spell::consolidate_conversation_memories;
pub use memory_spell::load_memories_for_prompt;
use plan_spell::PlanSpell;
use sandbox_spell::SandboxSpell;
use skill_spell::SkillSpell;
use sourcegraph_spell::search_code;
use tavily_spell::TavilySpell;

use std::sync::Arc;
use uuid::Uuid;

use crate::config::{McpCatalogEntry, ModelConfig};
use crate::db::Db;
use crate::embedding::EmbeddingClient;
use crate::prompt::PromptEngine;
use manage_mcp_spell::ManageMcpSpell;
use spawn_spell::SpawnSpell;
use ui_spells::UiSpells;

// ── Spell factory ─────────────────────────────────────────────────────────────

/// All runtime dependencies required to build the full built-in spell bundle.
pub struct SpellDeps {
    pub prompt_engine: PromptEngine,
    pub current_date: String,
    // SpawnSpell
    pub cheap_model: ModelConfig,
    pub history: Vec<agentix::Message>,
    // HistorySpell
    pub db: Db,
    pub embed: EmbeddingClient,
    pub conversation_id: Uuid,
    // ManageMcpSpell
    pub pool: sqlx::PgPool,
    pub user_id: Uuid,
    pub sandbox: Arc<crate::sandbox::SandboxManager>,
    pub mcp_catalog: Vec<McpCatalogEntry>,
    // TavilySpell
    pub tavily_api_key: Option<String>,
    // GenerateImageSpell
    pub siliconflow_api_key: Option<String>,
    pub fal_api_key: Option<String>,
    pub http: reqwest::Client,
}

/// Build the complete built-in spell bundle from the given dependencies.
pub fn build_all_spells(deps: SpellDeps) -> impl agentix::Tool {
    // Build the subagent tool bundle first (no SpawnSpell — avoids infinite recursion).
    let subagent_tavily: Option<TavilySpell> =
        deps.tavily_api_key.as_deref().map(|k| TavilySpell {
            api_key: k.to_string(),
            http: deps.http.clone(),
        });

    let mut subagent_bundle = agentix::ToolBundle::new();
    subagent_bundle.push(
        SkillSpell {
            pool: deps.pool.clone(),
            user_id: deps.user_id,
        } + search_code
            + HistorySpell {
                db: deps.db.clone(),
                embedding: deps.embed.clone(),
                conversation_id: deps.conversation_id,
            }
            + PlanSpell {
                pool: deps.pool.clone(),
                conversation_id: deps.conversation_id,
            }
            + MemorySpell {
                pool: deps.pool.clone(),
                user_id: deps.user_id,
                conversation_id: deps.conversation_id,
                embed: deps.embed.clone(),
            },
    );
    if let Some(t) = subagent_tavily {
        subagent_bundle.push(t);
    }
    let subagent_tools: Arc<dyn agentix::Tool> = Arc::new(subagent_bundle);

    let bundle = SkillSpell {
        pool: deps.pool.clone(),
        user_id: deps.user_id,
    } + SandboxSpell {
        sandbox: deps.sandbox.clone(),
        user_id: deps.user_id,
        conversation_id: deps.conversation_id,
    } + AstSpell
        + UiSpells {
            user_id: deps.user_id,
            conversation_id: deps.conversation_id,
            sandbox: deps.sandbox.clone(),
        }
        + search_code
        + SpawnSpell {
            cheap_model: deps.cheap_model,
            fresh_prompt: deps.prompt_engine.build_subagent(false, &deps.current_date),
            fork_prompt: deps.prompt_engine.build_subagent(true, &deps.current_date),
            tools: subagent_tools,
            history: deps.history,
        }
        + HistorySpell {
            db: deps.db,
            embedding: deps.embed.clone(),
            conversation_id: deps.conversation_id,
        }
        + ManageMcpSpell {
            pool: deps.pool.clone(),
            conversation_id: deps.conversation_id,
            user_id: deps.user_id,
            sandbox: deps.sandbox.clone(),
            catalog: deps.mcp_catalog,
        }
        + PlanSpell {
            pool: deps.pool.clone(),
            conversation_id: deps.conversation_id,
        }
        + MemorySpell {
            pool: deps.pool.clone(),
            user_id: deps.user_id,
            conversation_id: deps.conversation_id,
            embed: deps.embed,
        }
        + AdminSpells {
            pool: deps.pool,
            conversation_id: deps.conversation_id,
        };

    let mut tb = agentix::ToolBundle::new();
    tb.push(bundle);
    if let Some(api_key) = deps.tavily_api_key {
        tb.push(TavilySpell {
            api_key,
            http: deps.http.clone(),
        });
    }
    if deps.siliconflow_api_key.is_some() || deps.fal_api_key.is_some() {
        tb.push(GenerateImageSpell {
            siliconflow_api_key: deps.siliconflow_api_key,
            fal_api_key: deps.fal_api_key,
            http: deps.http.clone(),
            sandbox: deps.sandbox.clone(),
            user_id: deps.user_id,
            conversation_id: deps.conversation_id,
        });
    }
    tb
}

#[cfg(test)]
mod overhead_estimate {
    use super::*;
    use crate::config::Provider;
    use agentix::Tool;
    use sqlx::postgres::PgPoolOptions;
    use std::path::PathBuf;
    use tiktoken_rs::cl100k_base;

    fn count(bpe: &tiktoken_rs::CoreBPE, s: &str) -> usize {
        bpe.encode_with_special_tokens(s).len()
    }

    /// Run with: cargo test -p familiar overhead_estimate -- --nocapture
    #[tokio::test]
    async fn estimate_static_overhead() {
        let bpe = cl100k_base().expect("load BPE");

        // ── System prompt ────────────────────────────────────────────────
        let engine = PromptEngine::new();
        let main_with_mem = engine.build_main(true);
        let main_no_mem = engine.build_main(false);
        let main_mem_tokens = count(&bpe, &main_with_mem);
        let main_nomem_tokens = count(&bpe, &main_no_mem);

        // Skills list line that worker.rs appends
        let skills_block: String = {
            let mut s = String::from("\n\n可用 Skills（需要时调用 load_skill 获取详细指令）：\n");
            let mut entries: Vec<String> = crate::prompt::BUNDLED_SKILLS
                .iter()
                .map(|(name, desc, _)| format!("- {name}: {desc}"))
                .collect();
            entries.sort();
            s.push_str(&entries.join("\n"));
            s
        };
        let skills_tokens = count(&bpe, &skills_block);

        // ── Tool schemas ────────────────────────────────────────────────
        // PgPool::connect_lazy doesn't actually connect — the spells just need
        // a pool handle to be constructed.
        let pool = PgPoolOptions::new()
            .connect_lazy("postgres://stub:stub@localhost/stub")
            .expect("lazy pool");
        let sandbox = Arc::new(crate::sandbox::SandboxManager::new(PathBuf::from(
            "/tmp/overhead-stub",
        )));
        let db = crate::db::Db::new(pool.clone(), Arc::clone(&sandbox));
        let embed = crate::embedding::EmbeddingClient::new(
            String::from("stub"),
            String::from("http://stub"),
            String::from("stub"),
        );
        let cheap_model = ModelConfig {
            api_key: "stub".into(),
            api_base: "http://stub".into(),
            name: "stub".into(),
            provider: Provider::DeepSeek,
            extra_body: Default::default(),
            max_tokens: None,
            kind: "api".into(),
            compact_trigger_tokens: 200_000,
            compact_tail_tokens: 16_000,
            reasoning_effort: None,
        };
        let deps = SpellDeps {
            prompt_engine: PromptEngine::new(),
            current_date: "2026-04-29".into(),
            cheap_model,
            history: vec![],
            db,
            embed,
            conversation_id: Uuid::new_v4(),
            pool,
            user_id: Uuid::new_v4(),
            sandbox,
            mcp_catalog: vec![],
            tavily_api_key: Some("stub".into()),
            siliconflow_api_key: Some("stub".into()),
            fal_api_key: None,
            http: reqwest::Client::new(),
        };

        let bundle = build_all_spells(deps);
        let tools = bundle.raw_tools();

        let mut total_tool_tokens: usize = 0;
        let mut total_tool_full: usize = 0;
        let mut rows: Vec<(String, usize, usize, usize)> = Vec::new();
        for t in &tools {
            let name_t = count(&bpe, &t.function.name);
            let desc_t = t
                .function
                .description
                .as_ref()
                .map(|d| count(&bpe, d))
                .unwrap_or(0);
            let params_json = serde_json::to_string(&t.function.parameters).unwrap_or_default();
            let params_t = count(&bpe, &params_json);
            let inner = name_t + desc_t + params_t;
            // Full wire form is `{"type":"function","function":{...}}` —
            // tokenise the actual JSON the provider receives.
            let full_json = serde_json::to_string(t).unwrap_or_default();
            let full_t = count(&bpe, &full_json);
            total_tool_tokens += inner;
            total_tool_full += full_t;
            rows.push((t.function.name.clone(), name_t, desc_t + params_t, full_t));
        }
        rows.sort_by(|a, b| b.3.cmp(&a.3));

        // ── First user message from messages.json (real-world calibration) ──
        let user_msg_text = "[2026-04-28 16:15 UTC] Algorithm 1 Optimization and Densification\n\
            𝑤, ℎ: width and height of the training images\n\
            𝑀 ← SfM Points ⊲ Positions\n\
            𝑆,𝐶, 𝐴 ← InitAttributes() ⊲ Covariances, Colors, Opacities\n\
            𝑖 ← 0 ⊲ Iteration Count\n\
            while not converged do\n\
            𝑉 , ˆ𝐼 ← SampleTrainingView() ⊲ Camera 𝑉 and Image\n\
            𝐼 ← Rasterize(𝑀, 𝑆,𝐶, 𝐴, 𝑉 ) ⊲ Alg. 2\n\
            𝐿 ← Loss(𝐼, ˆ𝐼) ⊲ Loss\n\
            𝑀, 𝑆,𝐶, 𝐴 ← Adam(∇𝐿) ⊲ Backprop & Step\n\
            if IsRefinementIteration(𝑖) then\n\
            for all Gaussians (𝜇, Σ,𝑐, 𝛼) in (𝑀, 𝑆,𝐶, 𝐴) do\n\
            if 𝛼 < 𝜖 or IsTooLarge(𝜇, Σ) then ⊲ Pruning\n\
            RemoveGaussian()\n\
            end if\n\
            if ∇𝑝𝐿 > 𝜏𝑝 then ⊲ Densification\n\
            if ∥𝑆 ∥ > 𝜏𝑆 then ⊲ Over-reconstruction\n\
            SplitGaussian(𝜇, Σ,𝑐, 𝛼)\n\
            else ⊲ Under-reconstruction\n\
            CloneGaussian(𝜇, Σ,𝑐, 𝛼)\n\
            end if\n\
            end if\n\
            end for\n\
            end if\n\
            𝑖 ← 𝑖 + 1\n\
            end while\n\
            什么意思";
        let user_msg_chars = user_msg_text.chars().count();
        let user_msg_bytes = user_msg_text.len();
        let user_msg_tokens = count(&bpe, user_msg_text);

        println!("\n========== STATIC PROMPT + TOOL OVERHEAD (cl100k_base) ==========\n");
        println!("System prompt (main_base + memory section):");
        println!("  build_main(has_memory=true):  {main_mem_tokens:>6} tokens");
        println!("  build_main(has_memory=false): {main_nomem_tokens:>6} tokens");
        println!("Skills list block: {skills_tokens:>6} tokens  ({} entries)", crate::prompt::BUNDLED_SKILLS.len());
        println!();
        println!("Tool schemas: {} tools", tools.len());
        println!("  inner (name + desc + params JSON): {total_tool_tokens:>6} tokens");
        println!("  full wire ({{\"type\":..., \"function\":...}}): {total_tool_full:>6} tokens");
        println!();
        println!("{:<32} {:>6} {:>10} {:>8}", "tool_name", "name_t", "desc+schema", "wire");
        for (name, n, ds, t) in &rows {
            println!("{:<32} {:>6} {:>10} {:>8}", name, n, ds, t);
        }
        println!();
        println!("First user message (#18044 from messages.json):");
        println!("  {user_msg_chars} chars / {user_msg_bytes} bytes / {user_msg_tokens} tokens (cl100k)");
        println!("  → math italic glyphs (𝑤,𝑀,𝛼…) cost ~3+ tokens each");
        println!();
        let with_mem_total = main_mem_tokens + skills_tokens + total_tool_full + user_msg_tokens + 4;
        let no_mem_total = main_nomem_tokens + skills_tokens + total_tool_full + user_msg_tokens + 4;
        println!("Total estimated input on turn 1:");
        println!("  with memory section:    {with_mem_total:>6} tokens");
        println!("  without memory section: {no_mem_total:>6} tokens");
        println!();
        println!("Observed (messages.json id=18045): 29,514 tokens");
        println!("Discrepancy reflects: (a) Anthropic's tokenizer vs cl100k_base (~10-15% diff on Chinese)");
        println!("                     (b) any provider-side overhead per tool we don't see locally");
    }

    /// Build SpellDeps + bundle, return (system_prompt, tools, local_estimate).
    /// Used by both the claude-code and deepseek live probes.
    fn make_static_artefacts() -> (String, Vec<agentix::ToolDefinition>, (usize, usize)) {
        let engine = PromptEngine::new();
        let system_prompt = engine.build_main(false);
        let pool = PgPoolOptions::new()
            .connect_lazy("postgres://stub:stub@localhost/stub")
            .expect("lazy pool");
        let sandbox = Arc::new(crate::sandbox::SandboxManager::new(PathBuf::from(
            "/tmp/overhead-stub",
        )));
        let db = crate::db::Db::new(pool.clone(), Arc::clone(&sandbox));
        let embed = crate::embedding::EmbeddingClient::new(
            String::from("stub"),
            String::from("http://stub"),
            String::from("stub"),
        );
        let cheap_model = ModelConfig {
            api_key: "stub".into(),
            api_base: "http://stub".into(),
            name: "stub".into(),
            provider: Provider::DeepSeek,
            extra_body: Default::default(),
            max_tokens: None,
            kind: "api".into(),
            compact_trigger_tokens: 200_000,
            compact_tail_tokens: 16_000,
            reasoning_effort: None,
        };
        let deps = SpellDeps {
            prompt_engine: PromptEngine::new(),
            current_date: "2026-04-30".into(),
            cheap_model,
            history: vec![],
            db,
            embed,
            conversation_id: Uuid::new_v4(),
            pool,
            user_id: Uuid::new_v4(),
            sandbox,
            mcp_catalog: vec![],
            tavily_api_key: Some("stub".into()),
            siliconflow_api_key: Some("stub".into()),
            fal_api_key: None,
            http: reqwest::Client::new(),
        };
        let bundle = build_all_spells(deps);
        let tools = bundle.raw_tools();
        let bpe = cl100k_base().expect("load BPE");
        let local_sys = bpe.encode_with_special_tokens(&system_prompt).len();
        let local_tools: usize = tools
            .iter()
            .map(|t| {
                bpe.encode_with_special_tokens(&serde_json::to_string(t).unwrap_or_default())
                    .len()
            })
            .sum();
        (system_prompt, tools, (local_sys, local_tools))
    }

    /// Drives the real `claude` CLI via `Request::claude_code()` to measure the
    /// actual server-side input token count for [system_prompt + tool defs +
    /// minimal user message]. Captures every `LlmEvent::Usage` so we can also
    /// see whether claude-code emits multiple usage events per turn (the
    /// "double-accumulation" hypothesis for compaction triggers).
    ///
    /// Run with:
    ///   cargo test -p familiar overhead_real -- --ignored --nocapture
    ///
    /// Requires `claude` CLI on PATH and authenticated.
    #[tokio::test]
    #[ignore = "live: shells out to `claude` CLI; run manually"]
    async fn estimate_real_overhead() {
        use agentix::{LlmEvent, Message, Request, UserContent};
        use futures::StreamExt;

        // ── Build the same artefacts worker.rs would feed to the model ──
        let engine = PromptEngine::new();
        let system_prompt = engine.build_main(false); // no memory section here

        let pool = PgPoolOptions::new()
            .connect_lazy("postgres://stub:stub@localhost/stub")
            .expect("lazy pool");
        let sandbox = Arc::new(crate::sandbox::SandboxManager::new(PathBuf::from(
            "/tmp/overhead-stub",
        )));
        let db = crate::db::Db::new(pool.clone(), Arc::clone(&sandbox));
        let embed = crate::embedding::EmbeddingClient::new(
            String::from("stub"),
            String::from("http://stub"),
            String::from("stub"),
        );
        let cheap_model = ModelConfig {
            api_key: "stub".into(),
            api_base: "http://stub".into(),
            name: "stub".into(),
            provider: Provider::DeepSeek,
            extra_body: Default::default(),
            max_tokens: None,
            kind: "api".into(),
            compact_trigger_tokens: 200_000,
            compact_tail_tokens: 16_000,
            reasoning_effort: None,
        };
        let deps = SpellDeps {
            prompt_engine: PromptEngine::new(),
            current_date: "2026-04-30".into(),
            cheap_model,
            history: vec![],
            db,
            embed,
            conversation_id: Uuid::new_v4(),
            pool,
            user_id: Uuid::new_v4(),
            sandbox,
            mcp_catalog: vec![],
            tavily_api_key: Some("stub".into()),
            siliconflow_api_key: Some("stub".into()),
            fal_api_key: None,
            http: reqwest::Client::new(),
        };
        let bundle = build_all_spells(deps);
        let tools = bundle.raw_tools();

        // Tiny user message — keep input near the static baseline.
        let user_text = "ping";
        let messages = vec![Message::User(vec![UserContent::Text {
            text: user_text.into(),
        }])];

        // Local cl100k baseline for comparison.
        let bpe = cl100k_base().expect("load BPE");
        let local_sys = bpe.encode_with_special_tokens(&system_prompt).len();
        let local_tools: usize = tools
            .iter()
            .map(|t| {
                bpe.encode_with_special_tokens(&serde_json::to_string(t).unwrap_or_default())
                    .len()
            })
            .sum();
        let local_user = bpe.encode_with_special_tokens(user_text).len();

        println!("\n========== LOCAL cl100k ESTIMATE ==========");
        println!("system_prompt (no memory): {local_sys} tokens");
        println!("tools (full wire JSON):     {local_tools} tokens ({} tools)", tools.len());
        println!("user msg ({user_text:?}):           {local_user} tokens");
        println!("local total estimate:       {} tokens", local_sys + local_tools + local_user);
        println!();

        // ── Build & stream the request ───────────────────────────────────
        let req = Request::claude_code()
            .model("claude-opus-4-5") // adjust as needed; needs to match what `claude` accepts
            .system_prompt(system_prompt.clone())
            .messages(messages)
            .tools(tools.clone())
            .max_tokens(64); // tiny output to keep cost minimal

        let http = reqwest::Client::new();
        let mut stream = match req.stream(&http).await {
            Ok(s) => s,
            Err(e) => panic!("claude-code stream open failed: {e}"),
        };

        println!("========== LIVE EVENTS FROM `claude` CLI ==========");
        let mut usage_events: Vec<agentix::types::UsageStats> = Vec::new();
        let mut other_event_kinds: Vec<&'static str> = Vec::new();
        let mut tokens_collected = String::new();

        while let Some(ev) = stream.next().await {
            match ev {
                LlmEvent::Usage(u) => {
                    println!(
                        "[Usage #{}] prompt={} cache_read={} cache_creation={} completion={} total={}",
                        usage_events.len() + 1,
                        u.prompt_tokens,
                        u.cache_read_tokens,
                        u.cache_creation_tokens,
                        u.completion_tokens,
                        u.total_tokens
                    );
                    usage_events.push(u);
                }
                LlmEvent::Token(t) => {
                    tokens_collected.push_str(&t);
                }
                LlmEvent::Reasoning(_) => other_event_kinds.push("Reasoning"),
                LlmEvent::ToolCall(_) => other_event_kinds.push("ToolCall"),
                LlmEvent::ToolCallChunk(_) => other_event_kinds.push("ToolCallChunk"),
                LlmEvent::AssistantState(_) => other_event_kinds.push("AssistantState"),
                LlmEvent::Done => other_event_kinds.push("Done"),
                LlmEvent::Error(e) => {
                    println!("[Error] {e}");
                    other_event_kinds.push("Error");
                }
                _ => other_event_kinds.push("Other"),
            }
        }

        println!();
        println!("========== SUMMARY (claude-code) ==========");
        println!("Total Usage events received: {}", usage_events.len());
        if usage_events.len() > 1 {
            println!("⚠  Multiple Usage events → worker.rs `usage += u` would DOUBLE-COUNT");
            let total_prompt: usize = usage_events.iter().map(|u| u.prompt_tokens).sum();
            let total_cache_read: usize = usage_events.iter().map(|u| u.cache_read_tokens).sum();
            let total_cache_creation: usize = usage_events.iter().map(|u| u.cache_creation_tokens).sum();
            println!(
                "  accumulated prompt={total_prompt} cache_read={total_cache_read} cache_creation={total_cache_creation}"
            );
            let single_max = usage_events
                .iter()
                .map(|u| u.prompt_tokens + u.cache_read_tokens + u.cache_creation_tokens)
                .max()
                .unwrap_or(0);
            println!("  max single-event total input: {single_max}");
            let summed_total = total_prompt + total_cache_read + total_cache_creation;
            println!(
                "  ratio (accumulated / max single): {:.2}",
                summed_total as f64 / single_max.max(1) as f64
            );
        }
        println!("Other event kinds seen: {:?}", other_event_kinds);
        println!("Sampled output: {:?}", tokens_collected.chars().take(100).collect::<String>());
    }

    /// Same probe but against DeepSeek HTTP API. Lets us isolate whether the
    /// inflated input number is (a) Claude/Anthropic's tokenizer being denser
    /// than cl100k for tool schemas, or (b) `claude` CLI injecting extra context
    /// (CLAUDE.md, internal hooks, etc.) on top of what we send.
    ///
    /// Run with:
    ///   DEEPSEEK_API_KEY=sk-... cargo test -p familiar estimate_real_overhead_deepseek -- --ignored --nocapture
    #[tokio::test]
    #[ignore = "live: hits DeepSeek API; requires DEEPSEEK_API_KEY env var"]
    async fn estimate_real_overhead_deepseek() {
        use agentix::{LlmEvent, Message, Provider, Request, UserContent};
        use futures::StreamExt;

        let key = std::env::var("DEEPSEEK_API_KEY").expect("set DEEPSEEK_API_KEY");

        let (system_prompt, tools, (local_sys, local_tools)) = make_static_artefacts();
        let user_text = "ping";
        let local_user = cl100k_base()
            .unwrap()
            .encode_with_special_tokens(user_text)
            .len();

        println!("\n========== LOCAL cl100k ESTIMATE ==========");
        println!("system_prompt (no memory): {local_sys} tokens");
        println!("tools (full wire JSON):     {local_tools} tokens ({} tools)", tools.len());
        println!("user msg ({user_text:?}):           {local_user} tokens");
        println!("local total estimate:       {} tokens", local_sys + local_tools + local_user);
        println!();

        let messages = vec![Message::User(vec![UserContent::Text {
            text: user_text.into(),
        }])];

        let req = Request::new(Provider::DeepSeek, key)
            .model("deepseek-chat")
            .system_prompt(system_prompt)
            .messages(messages)
            .tools(tools)
            .max_tokens(64);

        let http = reqwest::Client::new();
        let mut stream = match req.stream(&http).await {
            Ok(s) => s,
            Err(e) => panic!("DeepSeek stream open failed: {e}"),
        };

        println!("========== LIVE EVENTS FROM DeepSeek API ==========");
        let mut usage_events: Vec<agentix::types::UsageStats> = Vec::new();
        let mut other_event_kinds: Vec<&'static str> = Vec::new();
        let mut tokens_collected = String::new();

        while let Some(ev) = stream.next().await {
            match ev {
                LlmEvent::Usage(u) => {
                    println!(
                        "[Usage #{}] prompt={} cache_read={} cache_creation={} completion={} total={}",
                        usage_events.len() + 1,
                        u.prompt_tokens,
                        u.cache_read_tokens,
                        u.cache_creation_tokens,
                        u.completion_tokens,
                        u.total_tokens
                    );
                    usage_events.push(u);
                }
                LlmEvent::Token(t) => tokens_collected.push_str(&t),
                LlmEvent::Reasoning(_) => other_event_kinds.push("Reasoning"),
                LlmEvent::ToolCall(_) => other_event_kinds.push("ToolCall"),
                LlmEvent::ToolCallChunk(_) => other_event_kinds.push("ToolCallChunk"),
                LlmEvent::AssistantState(_) => other_event_kinds.push("AssistantState"),
                LlmEvent::Done => other_event_kinds.push("Done"),
                LlmEvent::Error(e) => {
                    println!("[Error] {e}");
                    other_event_kinds.push("Error");
                }
                _ => other_event_kinds.push("Other"),
            }
        }

        println!();
        println!("========== SUMMARY (DeepSeek) ==========");
        println!("Total Usage events received: {}", usage_events.len());
        if !usage_events.is_empty() {
            let last = usage_events.last().unwrap();
            let real_input = last.prompt_tokens; // DeepSeek prompt_tokens = total input
            let local_total = local_sys + local_tools + local_user;
            println!(
                "Real prompt_tokens (last event): {} | local cl100k estimate: {} | ratio {:.2}",
                real_input,
                local_total,
                real_input as f64 / local_total.max(1) as f64
            );
        }
        println!("Other event kinds seen: {:?}", other_event_kinds);
        println!("Sampled output: {:?}", tokens_collected.chars().take(100).collect::<String>());
    }
}
