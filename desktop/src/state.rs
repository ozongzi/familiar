// Shared app state. We use Dioxus signals so any change re-renders consumers.

use dioxus::prelude::*;

use crate::storage::config::Config;
use crate::storage::conversation::{Conversation, Meta};

#[derive(Clone, Copy)]
pub struct AppState {
    pub config: Signal<Config>,
    pub conversation: Signal<Option<Conversation>>,
    pub conversation_list: Signal<Vec<Meta>>,
    pub generating: Signal<bool>,
    pub error: Signal<Option<String>>,
}

impl AppState {
    pub fn new() -> Self {
        let config = Signal::new(Config::load().unwrap_or_default());
        let conversation_list = Signal::new(Conversation::list_all());
        Self {
            config,
            conversation: Signal::new(None),
            conversation_list,
            generating: Signal::new(false),
            error: Signal::new(None),
        }
    }

    pub fn refresh_list(&mut self) {
        self.conversation_list.set(Conversation::list_all());
    }
}
