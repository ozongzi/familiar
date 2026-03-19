use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager, State};
use tokio::process::{Child, Command};

struct TunnelState {
    child: Option<Child>,
}

type SharedTunnelState = Arc<Mutex<TunnelState>>;

// ── Tauri commands ────────────────────────────────────────────────────────────

#[tauri::command]
async fn start_tunnel(
    token: String,
    server_url: String,
    state: State<'_, SharedTunnelState>,
    app: AppHandle,
) -> Result<(), String> {
    stop_tunnel_inner(&state).await;

    let script_path = app
        .path()
        .resource_dir()
        .map_err(|e| e.to_string())?
        .join("tunnel-bridge.js");

    if !script_path.exists() {
        return Err(format!(
            "tunnel-bridge.js not found at {}",
            script_path.display()
        ));
    }

    log::info!("Starting tunnel bridge → {server_url}");

    let child = Command::new("node")
        .arg(&script_path)
        .env("FAMILIAR_TOKEN", &token)
        .env("FAMILIAR_SERVER", &server_url)
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("Failed to spawn node: {e}"))?;

    state.lock().unwrap().child = Some(child);
    Ok(())
}

#[tauri::command]
async fn stop_tunnel(state: State<'_, SharedTunnelState>) -> Result<(), String> {
    stop_tunnel_inner(&state).await;
    Ok(())
}

async fn stop_tunnel_inner(state: &SharedTunnelState) {
    let child = state.lock().unwrap().child.take();
    if let Some(mut c) = child {
        let _ = c.kill().await;
        log::info!("Tunnel bridge stopped");
    }
}

// ── App entry ─────────────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let tunnel_state: SharedTunnelState = Arc::new(Mutex::new(TunnelState { child: None }));

    tauri::Builder::default()
        .manage(tunnel_state)
        .plugin(tauri_plugin_store::Builder::default().build())
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![start_tunnel, stop_tunnel])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
