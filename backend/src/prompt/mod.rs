use tera::{Context, Tera};

// ── Embedded templates ────────────────────────────────────────────────────────

const MAIN_BASE: &str = include_str!("templates/main_base.md");
const MAIN_MEMORY: &str = include_str!("templates/main_memory.md");
const MAIN_COMPACT: &str = include_str!("templates/main_compact.md");
const SUB_BASE: &str = include_str!("templates/sub_base.md");
const SUB_FRESH: &str = include_str!("templates/sub_fresh.md");
const SUB_FORK: &str = include_str!("templates/sub_fork.md");

// ── Main prompt template ──────────────────────────────────────────────────────
//
// Variables: current_date, has_memory, has_compact, compact_summary,
//            tpl_memory, tpl_compact
const MAIN_TEMPLATE: &str = r#"{{ base }}
{% if has_memory %}

---

{{ tpl_memory }}
{% endif %}
{% if has_compact %}

---

{{ tpl_compact }}
{{ compact_summary }}
{% endif %}"#;

// ── Subagent prompt template ──────────────────────────────────────────────────
//
// Variables: base, mode ("fresh" | "fork"), tpl_fresh, tpl_fork
const SUBAGENT_TEMPLATE: &str = r#"{{ base }}
{% if mode == "fork" %}
{{ tpl_fork }}
{% else %}
{{ tpl_fresh }}
{% endif %}"#;

// ── Engine ────────────────────────────────────────────────────────────────────

/// Renders system prompts via embedded Tera templates.
///
/// `Tera` is `Send + Sync`, so `PromptEngine` can be held across `.await`
/// points inside `tokio::spawn` futures without issue.
pub struct PromptEngine {
    tera: Tera,
}

impl PromptEngine {
    pub fn new() -> Self {
        let mut tera = Tera::default();
        tera.add_raw_template("main", MAIN_TEMPLATE)
            .expect("main prompt template is invalid");
        tera.add_raw_template("subagent", SUBAGENT_TEMPLATE)
            .expect("subagent prompt template is invalid");
        Self { tera }
    }

    /// Build the main agent system prompt.
    ///
    /// - `has_memory`: whether there are stored memories to inject
    /// - `has_compact`: whether a compact summary exists for this conversation
    /// - `compact_summary`: the summary text (only used when `has_compact`)
    /// - `current_date`: today's date string
    pub fn build_main(
        &self,
        has_memory: bool,
        has_compact: bool,
        compact_summary: &str,
        current_time: &str,
    ) -> String {
        let base = MAIN_BASE.replace("{{ current_time }}", current_time);
        let mut ctx = Context::new();
        ctx.insert("base", &base);
        ctx.insert("has_memory", &has_memory);
        ctx.insert("has_compact", &has_compact);
        ctx.insert("compact_summary", compact_summary);
        ctx.insert("tpl_memory", MAIN_MEMORY);
        ctx.insert("tpl_compact", MAIN_COMPACT);

        self.tera.render("main", &ctx).unwrap_or_else(|e| {
            tracing::warn!("main prompt template error: {e}");
            base
        })
    }

    /// Build the subagent system prompt.
    ///
    /// - `fork`: `true` = fork mode (inherited context), `false` = fresh mode
    pub fn build_subagent(&self, fork: bool, _current_time: &str) -> String {
        let mode = if fork { "fork" } else { "fresh" };
        let mut ctx = Context::new();
        ctx.insert("base", SUB_BASE);
        ctx.insert("mode", mode);
        ctx.insert("tpl_fresh", SUB_FRESH);
        ctx.insert("tpl_fork", SUB_FORK);

        self.tera.render("subagent", &ctx).unwrap_or_else(|e| {
            tracing::warn!("subagent prompt template error: {e}");
            format!("{SUB_BASE}\n{}", if fork { SUB_FORK } else { SUB_FRESH })
        })
    }
}

impl Default for PromptEngine {
    fn default() -> Self {
        Self::new()
    }
}
