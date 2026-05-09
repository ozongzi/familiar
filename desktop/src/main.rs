// Familiar — local desktop client.
//
// No server, no account, no database. All state lives as files under
// the user's data directory. See storage::paths.

mod app;
mod components;
mod llm;
mod pages;
mod sandbox;
mod state;
mod storage;
mod tools;

use dioxus::desktop::{Config, WindowBuilder};
use dioxus::prelude::*;
use tracing_subscriber::EnvFilter;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,familiar_desktop=debug")),
        )
        .init();

    if let Err(e) = storage::paths::ensure_layout() {
        tracing::error!(?e, "failed to create data directories");
    }

    let window = WindowBuilder::new()
        .with_title("Familiar")
        .with_inner_size(dioxus::desktop::LogicalSize::new(1100.0, 760.0))
        .with_min_inner_size(dioxus::desktop::LogicalSize::new(640.0, 480.0));

    LaunchBuilder::desktop()
        .with_cfg(Config::new().with_window(window))
        .launch(app::App);
}
