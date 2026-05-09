use std::path::PathBuf;

use anyhow::{Context, Result};
use directories::ProjectDirs;

pub fn data_dir() -> PathBuf {
    if let Ok(p) = std::env::var("FAMILIAR_HOME") {
        return PathBuf::from(p);
    }
    ProjectDirs::from("dev", "familiar", "familiar")
        .map(|d| d.data_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from(".familiar"))
}

pub fn config_path() -> PathBuf {
    data_dir().join("config.toml")
}

pub fn conversations_dir() -> PathBuf {
    data_dir().join("conversations")
}

pub fn memories_dir() -> PathBuf {
    data_dir().join("memories")
}

pub fn skills_dir() -> PathBuf {
    data_dir().join("skills")
}

pub fn workspaces_dir() -> PathBuf {
    data_dir().join("workspaces")
}

pub fn workspace_for(conversation_id: &str) -> PathBuf {
    workspaces_dir().join(conversation_id)
}

pub fn ensure_layout() -> Result<()> {
    for dir in [
        data_dir(),
        conversations_dir(),
        memories_dir(),
        skills_dir(),
        workspaces_dir(),
    ] {
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("create {}", dir.display()))?;
    }
    Ok(())
}
