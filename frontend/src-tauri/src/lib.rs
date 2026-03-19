use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_shell::{ShellExt, process::CommandChild};

// ── 状态 ──────────────────────────────────────────────────────────────────────

struct TunnelState {
    child: Option<CommandChild>,
}

type SharedTunnelState = Arc<Mutex<TunnelState>>;

// ── Tauri commands ────────────────────────────────────────────────────────────

/// 登录完成后由前端调用，用 token 启动隧道桥接进程
#[tauri::command]
async fn start_tunnel(
    token: String,
    server_url: String,
    state: State<'_, SharedTunnelState>,
    app: AppHandle,
) -> Result<(), String> {
    stop_tunnel_inner(&state);

    let resource_dir = app
        .path()
        .resource_dir()
        .map_err(|e| e.to_string())?;

    let script_path = resource_dir.join("tunnel-bridge.cjs");

    if !script_path.exists() {
        return Err(format!(
            "tunnel-bridge.cjs 不存在: {}",
            script_path.display()
        ));
    }

    log::info!("启动隧道桥接 → {server_url}");
    log::info!("resource_dir: {}", resource_dir.display());
    log::info!("script_path: {}", script_path.display());

    let (mut rx, child) = app
        .shell()
        .sidecar("node")
        .map_err(|e| format!("找不到 node sidecar: {e}"))?
        .args([script_path.to_string_lossy().as_ref()])
        .env("FAMILIAR_TOKEN", &token)
        .env("FAMILIAR_SERVER", &server_url)
        .env("FAMILIAR_RESOURCE_DIR", resource_dir.to_string_lossy().as_ref())
        .spawn()
        .map_err(|e| format!("无法启动 node 进程: {e}"))?;

    // 消费事件流，同时把输出写到日志
    tokio::spawn(async move {
        use tauri_plugin_shell::process::CommandEvent;
        while let Some(event) = rx.recv().await {
            match event {
                CommandEvent::Stdout(line) => {
                    log::info!("[tunnel] {}", String::from_utf8_lossy(&line));
                }
                CommandEvent::Stderr(line) => {
                    log::warn!("[tunnel] {}", String::from_utf8_lossy(&line));
                }
                CommandEvent::Error(e) => {
                    log::error!("[tunnel] 进程错误: {e}");
                }
                CommandEvent::Terminated(status) => {
                    log::info!("[tunnel] 进程退出: {:?}", status);
                    break;
                }
                _ => {}
            }
        }
    });

    state.lock().unwrap().child = Some(child);
    Ok(())
}

/// 登出后由前端调用，停止隧道桥接进程
#[tauri::command]
async fn stop_tunnel(state: State<'_, SharedTunnelState>) -> Result<(), String> {
    stop_tunnel_inner(&state);
    Ok(())
}

fn stop_tunnel_inner(state: &SharedTunnelState) {
    let child = state.lock().unwrap().child.take();
    if let Some(c) = child {
        let _ = c.kill();
        log::info!("隧道桥接已停止");
    }
}

// ── 入口 ──────────────────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let tunnel_state: SharedTunnelState = Arc::new(Mutex::new(TunnelState { child: None }));

    tauri::Builder::default()
        .manage(tunnel_state)
        .plugin(tauri_plugin_shell::init())
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
        .expect("运行 Tauri 应用时出错");
}
