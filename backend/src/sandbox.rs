use std::path::PathBuf;
use std::process::Command;
use tracing::{error, info};
use uuid::Uuid;

pub struct SandboxManager {
    /// Path as seen inside the familiar process (container or host).
    /// Used for all file I/O (read/write tools).
    base_path: PathBuf,
    /// Path as seen by the Docker daemon on the *host*.
    /// Used as the left-hand side of `-v <host_path>:/workspace` so that
    /// sandbox containers can mount the same directory.
    ///
    /// For bind-mount deployments this equals `base_path`.
    /// Set `HOST_ARTIFACTS_PATH` env var to override when they differ
    /// (e.g. running familiar inside Docker with a named volume).
    host_base_path: PathBuf,
}

impl SandboxManager {
    pub fn new(base_path: PathBuf) -> Self {
        let host_base_path = std::env::var("HOST_ARTIFACTS_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| base_path.clone());

        if !base_path.exists() {
            std::fs::create_dir_all(&base_path).unwrap_or_else(|e| {
                error!("Failed to create sandbox base path {:?}: {}", base_path, e);
            });
        }
        Self {
            base_path,
            host_base_path,
        }
    }

    pub fn get_conversation_dir(&self, user_id: Uuid, conversation_id: Uuid) -> PathBuf {
        self.base_path
            .join(user_id.to_string())
            .join(conversation_id.to_string())
    }

    fn get_host_conversation_dir(&self, user_id: Uuid, conversation_id: Uuid) -> PathBuf {
        self.host_base_path
            .join(user_id.to_string())
            .join(conversation_id.to_string())
    }

    fn container_name(conversation_id: Uuid) -> String {
        format!("familiar-sandbox-{}", conversation_id)
    }

    pub fn ensure_container(&self, user_id: Uuid, conversation_id: Uuid) -> Result<String, String> {
        let container_name = Self::container_name(conversation_id);
        let conv_dir = self.get_conversation_dir(user_id, conversation_id);
        // host_conv_dir is what Docker daemon sees — must be a real host path.
        let host_conv_dir = self.get_host_conversation_dir(user_id, conversation_id);

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
                        && s.success()
                    {
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
                        "--label",
                        "familiar.managed=true",
                        "--label",
                        &format!("familiar.user_id={user_id}"),
                        "--label",
                        &format!("familiar.conversation_id={conversation_id}"),
                        "-v",
                        &format!("{}:/workspace", host_conv_dir.to_str().unwrap()),
                        "-w",
                        "/workspace",
                        "--entrypoint",
                        "tail",
                        "familiar-sandbox:latest",
                        "-f",
                        "/dev/null",
                    ])
                    .status();

                if let Ok(s) = run_status
                    && s.success()
                {
                    return Ok(container_name);
                }
                Err("Failed to create container".into())
            }
        }
    }

    pub fn remove_conversation_resources(
        &self,
        user_id: Uuid,
        conversation_id: Uuid,
    ) -> Result<(), String> {
        let container_name = Self::container_name(conversation_id);

        let rm_status = Command::new("docker")
            .args(["rm", "-f", &container_name])
            .status();
        match rm_status {
            Ok(status) if status.success() => {}
            Ok(_) => {
                info!(
                    "Sandbox container {} did not exist or could not be removed",
                    container_name
                );
            }
            Err(err) => {
                error!(
                    "Failed to remove sandbox container {}: {}",
                    container_name, err
                );
            }
        }

        let conv_dir = self.get_conversation_dir(user_id, conversation_id);
        if conv_dir.exists() {
            std::fs::remove_dir_all(&conv_dir).map_err(|e| e.to_string())?;
        }

        Ok(())
    }

    #[allow(dead_code)]
    pub fn remove_container(&self, conversation_id: Uuid) {
        let container_name = Self::container_name(conversation_id);
        let rm_status = Command::new("docker")
            .args(["rm", "-f", &container_name])
            .status();
        match rm_status {
            Ok(status) if status.success() => {
                info!("Removed sandbox container {}", container_name);
            }
            Ok(_) => {
                info!(
                    "Sandbox container {} did not exist or could not be removed",
                    container_name
                );
            }
            Err(err) => {
                error!(
                    "Failed to remove sandbox container {}: {}",
                    container_name, err
                );
            }
        }
    }

    pub fn remove_user_resources(
        &self,
        user_id: Uuid,
        conversation_ids: &[Uuid],
    ) -> Result<(), String> {
        for &conversation_id in conversation_ids {
            self.remove_conversation_resources(user_id, conversation_id)?;
        }

        let user_dir = self.base_path.join(user_id.to_string());
        if user_dir.exists() {
            std::fs::remove_dir_all(&user_dir).map_err(|e| e.to_string())?;
        }

        Ok(())
    }

    pub fn wrap_mcp_command(
        &self,
        user_id: Uuid,
        conversation_id: Uuid,
        command: &str,
        args: &[&str],
    ) -> (String, Vec<String>) {
        let container_name = Self::container_name(conversation_id);

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
