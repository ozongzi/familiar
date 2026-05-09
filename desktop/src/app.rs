use dioxus::prelude::*;

use crate::pages::chat::ChatPage;
use crate::pages::settings::SettingsPage;
use crate::state::AppState;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum View {
    Chat,
    Settings,
}

#[component]
pub fn App() -> Element {
    use_context_provider(AppState::new);
    let view = use_signal(|| View::Chat);

    rsx! {
        style { {include_str!("assets/style.css")} }
        div { class: "app",
            Sidebar { view }
            div { class: "main",
                match view() {
                    View::Chat => rsx! { ChatPage {} },
                    View::Settings => rsx! { SettingsPage {} },
                }
            }
        }
    }
}

#[component]
fn Sidebar(view: Signal<View>) -> Element {
    let mut state = use_context::<AppState>();
    let conversations = state.conversation_list.read().clone();
    let current_id = state
        .conversation
        .read()
        .as_ref()
        .map(|c| c.meta.id.clone());

    rsx! {
        aside { class: "sidebar",
            div { class: "sidebar-header",
                button {
                    class: "new-btn",
                    onclick: move |_| {
                        let model = state.config.read().model.clone();
                        let conv = crate::storage::conversation::Conversation::new(&model);
                        let _ = conv.save();
                        state.conversation.set(Some(conv));
                        state.refresh_list();
                        view.set(View::Chat);
                    },
                    "+ 新对话"
                }
            }
            div { class: "conv-list",
                for meta in conversations.iter() {
                    {
                        let id = meta.id.clone();
                        let id_for_click = id.clone();
                        let id_for_del = id.clone();
                        let title = meta.title.clone();
                        let active = current_id.as_ref() == Some(&id);
                        let cls = if active { "conv-item active" } else { "conv-item" };
                        rsx! {
                            div {
                                key: "{id}",
                                class: "{cls}",
                                onclick: move |_| {
                                    if let Ok(c) = crate::storage::conversation::Conversation::load(&id_for_click) {
                                        state.conversation.set(Some(c));
                                        view.set(View::Chat);
                                    }
                                },
                                span { class: "conv-title", "{title}" }
                                button {
                                    class: "del-btn",
                                    onclick: move |evt| {
                                        evt.stop_propagation();
                                        let _ = crate::storage::conversation::Conversation::delete(&id_for_del);
                                        if state.conversation.read().as_ref().map(|c| &c.meta.id) == Some(&id_for_del) {
                                            state.conversation.set(None);
                                        }
                                        state.refresh_list();
                                    },
                                    "✕"
                                }
                            }
                        }
                    }
                }
            }
            div { class: "sidebar-footer",
                button {
                    class: "settings-btn",
                    onclick: move |_| view.set(View::Settings),
                    "⚙ 设置"
                }
            }
        }
    }
}
