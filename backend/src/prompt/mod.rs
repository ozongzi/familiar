use tera::{Context, Tera};

// ── Bundled app skills ────────────────────────────────────────────────────────

const SKILL_VISUALIZER_ART: &str = include_str!("skills/visualizer-art.md");
const SKILL_VISUALIZER_CHART: &str = include_str!("skills/visualizer-chart.md");
const SKILL_VISUALIZER_CORE: &str = include_str!("skills/visualizer-core.md");
const SKILL_VISUALIZER_DIAGRAM: &str = include_str!("skills/visualizer-diagram.md");
const SKILL_VISUALIZER_UI: &str = include_str!("skills/visualizer-ui.md");

/// Each entry: (name, description, raw_content_with_frontmatter).
/// These are compiled into the binary and available without a database lookup.
pub const BUNDLED_SKILLS: &[(&str, &str, &str)] = &[
    ("visualizer-art",     "插画和生成艺术规范，包括有机形状、重复纹理、径向对称和自由色彩使用。与其他模块不同，此模块允许自定义颜色和渐变。", SKILL_VISUALIZER_ART),
    ("visualizer-chart",   "数据图表规范，包括Chart.js配置、图例、数字格式化，以及D3 Choropleth地理地图的拓扑数据源和投影设置。", SKILL_VISUALIZER_CHART),
    ("visualizer-core",    "可视化系统核心规则，包括何时使用可视化、流式渲染约束、CSS变量、颜色系统和暗色模式要求。每次使用可视化系统前必须先加载此模块。", SKILL_VISUALIZER_CORE),
    ("visualizer-diagram", "SVG图表绘制规范，包括流程图、结构图和示意图三种类型的布局规则、节点样式、箭头标记和坐标计算。在绘制任何SVG图表前加载。", SKILL_VISUALIZER_DIAGRAM),
    ("visualizer-ui",      "UI组件和界面原型规范，包括卡片、表单、数据记录、选项对比等布局模式和设计token。", SKILL_VISUALIZER_UI),
];

// ── Embedded templates ────────────────────────────────────────────────────────

const MAIN_BASE: &str = include_str!("templates/main_base.md");
const MAIN_MEMORY: &str = include_str!("templates/main_memory.md");
const SUB_BASE: &str = include_str!("templates/sub_base.md");
const SUB_FRESH: &str = include_str!("templates/sub_fresh.md");
const SUB_FORK: &str = include_str!("templates/sub_fork.md");

// ── Main prompt template ──────────────────────────────────────────────────────
//
// Variables: base, has_memory, tpl_memory
const MAIN_TEMPLATE: &str = r#"{{ base }}
{% if has_memory %}

---

{{ tpl_memory }}
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
    /// - `current_date`: today's date string
    pub fn build_main(
        &self,
        has_memory: bool,
    ) -> String {
        let mut ctx = Context::new();
        ctx.insert("base", MAIN_BASE);
        ctx.insert("has_memory", &has_memory);
        ctx.insert("tpl_memory", MAIN_MEMORY);

        self.tera.render("main", &ctx).unwrap_or_else(|e| {
            tracing::warn!("main prompt template error: {e}");
            MAIN_BASE.to_string()
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
