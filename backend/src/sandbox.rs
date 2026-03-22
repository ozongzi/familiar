use std::path::PathBuf;
use std::process::Command;
use tracing::{error, info};
use uuid::Uuid;

pub struct SandboxManager {
    base_path: PathBuf,
}

impl SandboxManager {
    pub fn new(base_path: PathBuf) -> Self {
        // Ensure base_path exists
        if !base_path.exists() {
            std::fs::create_dir_all(&base_path).unwrap_or_else(|e| {
                error!("Failed to create sandbox base path {:?}: {}", base_path, e);
            });
        }
        Self { base_path }
    }

    pub fn get_conversation_dir(&self, user_id: Uuid, conversation_id: Uuid) -> PathBuf {
        self.base_path
            .join(user_id.to_string())
            .join(conversation_id.to_string())
    }

    pub fn ensure_container(&self, user_id: Uuid, conversation_id: Uuid) -> Result<String, String> {
        let container_name = format!("familiar-sandbox-{}", conversation_id);
        let conv_dir = self.get_conversation_dir(user_id, conversation_id);

        if !conv_dir.exists() {
            std::fs::create_dir_all(&conv_dir).map_err(|e| e.to_string())?;
        }

        // Check if container exists
        let status = Command::new("docker")
            .args(["inspect", "-f", "{{.State.Running}}", &container_name])
            .output();

        match status {
            Ok(output) if output.status.success() => {
                let running = String::from_utf8_lossy(&output.stdout).trim() == "true";
                if running {
                    Ok(container_name)
                } else {
                    info!("Starting existing container {}", container_name);
                    let start_status = Command::new("docker")
                        .args(["start", &container_name])
                        .status();
                    if let Ok(s) = start_status
                        && s.success() {
                            return Ok(container_name);
                        }
                    Err("Failed to start container".into())
                }
            }
            _ => {
                // Container doesn't exist, create and start it
                info!("Creating new sandbox container {}", container_name);

                let run_status = Command::new("docker")
                    .args([
                        "run",
                        "-d",
                        "--name",
                        &container_name,
                        "-v",
                        &format!("{}:/workspace", conv_dir.to_str().unwrap()),
                        "-w",
                        "/workspace",
                        "--restart",
                        "always",
                        "--entrypoint",
                        "tail",
                        "autocheck-mcp:latest",
                        "-f",
                        "/dev/null",
                    ])
                    .status();

                if let Ok(s) = run_status
                    && s.success() {
                        return Ok(container_name);
                    }
                Err("Failed to create container".into())
            }
        }
    }

    pub fn wrap_mcp_command(
        &self,
        user_id: Uuid,
        conversation_id: Uuid,
        command: &str,
        args: &[&str],
    ) -> (String, Vec<String>) {
        let container_name = format!("familiar-sandbox-{}", conversation_id);

        // Ensure container is running (best effort)
        let _ = self.ensure_container(user_id, conversation_id);

        let mut docker_args = vec![
            "exec".to_string(),
            "-i".to_string(),
            container_name,
            command.to_string(),
        ];

        for arg in args {
            docker_args.push(arg.to_string());
        }

        ("docker".to_string(), docker_args)
    }
}
