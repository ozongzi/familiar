use dioxus::prelude::*;
use pulldown_cmark::{Options, Parser, html};

use crate::storage::conversation::{Block, Role, Turn};

#[component]
pub fn TurnView(turn: Turn) -> Element {
    let role_label = match turn.role {
        Role::User => "你",
        Role::Assistant => "Familiar",
    };
    let role_class = match turn.role {
        Role::User => "turn turn-user",
        Role::Assistant => "turn turn-assistant",
    };
    rsx! {
        div { class: "{role_class}",
            div { class: "role-label", "{role_label}" }
            div { class: "blocks",
                for (idx, block) in turn.blocks.iter().enumerate() {
                    {
                        let key_str = format!("{idx}");
                        match block {
                            Block::Text { text } => rsx! {
                                div { key: "{key_str}", class: "block-text",
                                    dangerous_inner_html: "{render_md(text)}"
                                }
                            },
                            Block::ToolUse { name, input, .. } => {
                                let pretty = serde_json::to_string_pretty(input).unwrap_or_default();
                                rsx! {
                                    div { key: "{key_str}", class: "block-tool-use",
                                        div { class: "tool-header", "🔧 {name}" }
                                        pre { class: "tool-input", "{pretty}" }
                                    }
                                }
                            },
                            Block::ToolResult { content, is_error, .. } => {
                                let cls = if *is_error { "block-tool-result error" } else { "block-tool-result" };
                                rsx! {
                                    div { key: "{key_str}", class: "{cls}",
                                        div { class: "tool-header", "↩ result" }
                                        pre { class: "tool-output", "{content}" }
                                    }
                                }
                            },
                        }
                    }
                }
            }
        }
    }
}

fn render_md(src: &str) -> String {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_TASKLISTS);
    let parser = Parser::new_ext(src, opts);
    let mut out = String::new();
    html::push_html(&mut out, parser);
    out
}
