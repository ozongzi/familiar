use dioxus::prelude::*;

use crate::state::AppState;
use crate::storage::config::default_system_prompt;
use crate::storage::paths;

#[component]
pub fn SettingsPage() -> Element {
    let mut state = use_context::<AppState>();
    let cfg = state.config.read().clone();

    let mut api_key = use_signal(|| cfg.anthropic_api_key.clone());
    let mut model = use_signal(|| cfg.model.clone());
    let mut max_tokens = use_signal(|| cfg.max_tokens.to_string());
    let mut system_prompt = use_signal(|| cfg.system_prompt.clone());
    let mut saved = use_signal(|| false);

    let data_dir = paths::data_dir();

    rsx! {
        div { class: "settings",
            h2 { "设置" }
            p { class: "data-dir", "数据目录: {data_dir.display()}" }

            label {
                "Anthropic API Key"
                input {
                    r#type: "password",
                    value: "{api_key}",
                    oninput: move |e| { api_key.set(e.value()); saved.set(false); }
                }
            }

            label {
                "Model"
                input {
                    value: "{model}",
                    oninput: move |e| { model.set(e.value()); saved.set(false); }
                }
                small { "如 claude-sonnet-4-6 / claude-opus-4-7 / claude-haiku-4-5" }
            }

            label {
                "Max tokens"
                input {
                    r#type: "number",
                    value: "{max_tokens}",
                    oninput: move |e| { max_tokens.set(e.value()); saved.set(false); }
                }
            }

            label {
                "系统提示词"
                textarea {
                    rows: 10,
                    value: "{system_prompt}",
                    oninput: move |e| { system_prompt.set(e.value()); saved.set(false); }
                }
                button {
                    class: "link-btn",
                    onclick: move |_| {
                        system_prompt.set(default_system_prompt());
                        saved.set(false);
                    },
                    "↺ 恢复默认"
                }
            }

            div { class: "actions",
                button {
                    class: "save-btn",
                    onclick: move |_| {
                        let mut new_cfg = state.config.read().clone();
                        new_cfg.anthropic_api_key = api_key.read().clone();
                        new_cfg.model = model.read().clone();
                        new_cfg.max_tokens = max_tokens.read().parse().unwrap_or(8192);
                        new_cfg.system_prompt = system_prompt.read().clone();
                        if let Err(e) = new_cfg.save() {
                            state.error.set(Some(format!("保存失败: {e}")));
                        } else {
                            state.config.set(new_cfg);
                            saved.set(true);
                        }
                    },
                    "保存"
                }
                if saved() {
                    span { class: "saved-hint", "✓ 已保存" }
                }
            }
        }
    }
}
