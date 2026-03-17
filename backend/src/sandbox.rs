use std::process::Command;
use uuid::Uuid;
use tracing::{info, error};
use std::path::PathBuf;

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

    pub fn get_user_dir(&self, user_id: Uuid) -> PathBuf {
        self.base_path.join(user_id.to_string())
    }

    pub fn ensure_container(&self, user_id: Uuid) -> Result<String, String> {
        let container_name = format!("familiar-sandbox-{}", user_id);
        let user_dir = self.get_user_dir(user_id);

        if !user_dir.exists() {
            std::fs::create_dir_all(&user_dir).map_err(|e| e.to_string())?;
        }

        // Check if container exists
        let status = Command::new("docker")
            .args(["inspect", "-f", "{{.State.Running}}", &container_name])
            .output();

        match status {
            Ok(output) if output.status.success() => {
                let running = String::from_utf8_lossy(&output.stdout).trim() == "true";
                if running {
                    return Ok(container_name);
                } else {
                    info!("Starting existing container {}", container_name);
                    let start_status = Command::new("docker")
                        .args(["start", &container_name])
                        .status();
                    if let Ok(s) = start_status {
                        if s.success() {
                            return Ok(container_name);
                        }
                    }
                    return Err("Failed to start container".into());
                }
            }
            _ => {
                // Container doesn't exist, create and start it
                info!("Creating new sandbox container {}", container_name);
                
                let run_status = Command::new("docker")
                    .args([
                        "run",
                        "-d",
                        "--name", &container_name,
                        "-v", &format!("{}:/workspace", user_dir.to_str().unwrap()),
                        "-w", "/workspace",
                        "--restart", "always",
                        "node:20-slim",
                        "tail", "-f", "/dev/null"
                    ])
                    .status();

                if let Ok(s) = run_status {
                    if s.success() {
                        return Ok(container_name);
                    }
                }
                Err("Failed to create container".into())
            }
        }
    }

    pub fn wrap_mcp_command(&self, user_id: Uuid, command: &str, args: &[&str]) -> (String, Vec<String>) {
        let container_name = format!("familiar-sandbox-{}", user_id);
        
        // Ensure container is running (best effort)
        let _ = self.ensure_container(user_id);

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
