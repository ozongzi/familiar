use dioxus::prelude::*;

#[component]
pub fn Composer(disabled: bool, on_send: EventHandler<String>) -> Element {
    let mut text = use_signal(String::new);

    rsx! {
        div { class: "composer",
            textarea {
                class: "composer-input",
                placeholder: if disabled { "等待响应中…" } else { "输入消息（Enter 发送，Shift+Enter 换行）" },
                disabled: disabled,
                value: "{text}",
                oninput: move |evt| text.set(evt.value()),
                onkeydown: move |evt| {
                    if evt.key() == Key::Enter && !evt.modifiers().shift() {
                        evt.prevent_default();
                        let value = text.read().trim().to_string();
                        if !value.is_empty() {
                            text.set(String::new());
                            on_send.call(value);
                        }
                    }
                },
            }
            button {
                class: "send-btn",
                disabled: disabled,
                onclick: move |_| {
                    let value = text.read().trim().to_string();
                    if !value.is_empty() {
                        text.set(String::new());
                        on_send.call(value);
                    }
                },
                "发送"
            }
        }
    }
}
