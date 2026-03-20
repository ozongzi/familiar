use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_store::StoreExt;

#[cfg(not(target_os = "android"))]
use tauri_plugin_shell::{ShellExt, process::CommandChild};

// ── 状态 ──────────────────────────────────────────────────────────────────────

#[cfg(not(target_os = "android"))]
struct TunnelState {
    child: Option<CommandChild>,
}

#[cfg(target_os = "android")]
struct TunnelState {}

type SharedTunnelState = Arc<Mutex<TunnelState>>;

const LOCAL_MCP_STORE: &str = "local-mcps.json";
const LOCAL_MCP_KEY: &str = "mcps";

// ── Tauri commands ────────────────────────────────────────────────────────────

/// 登录完成后由前端调用，用 token 启动隧道桥接进程
#[tauri::command]
#[cfg(not(target_os = "android"))]
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

    // 读取本地 MCP 配置，序列化后传给 tunnel-bridge
    let local_mcps = load_local_mcps(&app);

    log::info!("启动隧道桥接 → {server_url}");
    log::info!("本地 MCP 数量: {}", local_mcps.as_array().map(|a| a.len()).unwrap_or(0));

    let (mut rx, child) = app
        .shell()
        .sidecar("node")
        .map_err(|e| format!("找不到 node sidecar: {e}"))?
        .args([script_path.to_string_lossy().as_ref()])
        .env("FAMILIAR_TOKEN", &token)
        .env("FAMILIAR_SERVER", &server_url)
        .env("FAMILIAR_RESOURCE_DIR", resource_dir.to_string_lossy().as_ref())
        .env("FAMILIAR_LOCAL_MCPS", serde_json::to_string(&local_mcps).unwrap_or_default())
        .spawn()
        .map_err(|e| format!("无法启动 node 进程: {e}"))?;

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
#[cfg(not(target_os = "android"))]
async fn stop_tunnel(state: State<'_, SharedTunnelState>) -> Result<(), String> {
    stop_tunnel_inner(&state);
    Ok(())
}

#[cfg(not(target_os = "android"))]
fn stop_tunnel_inner(state: &SharedTunnelState) {
    let child = state.lock().unwrap().child.take();
    if let Some(c) = child {
        let _ = c.kill();
        log::info!("隧道桥接已停止");
    }
}

#[cfg(target_os = "android")]
#[tauri::command]
async fn start_tunnel(_token: String, _server_url: String) -> Result<(), String> {
    Ok(()) // Android 暂不支持本地 MCP 隧道
}

#[cfg(target_os = "android")]
#[tauri::command]
async fn stop_tunnel() -> Result<(), String> {
    Ok(())
}

/// 读取本地 MCP 列表
#[tauri::command]
fn get_local_mcps(app: AppHandle) -> serde_json::Value {
    load_local_mcps(&app)
}

#[cfg(target_os = "android")]
#[tauri::command]
fn set_local_mcps(_app: AppHandle, _mcps: serde_json::Value) -> Result<(), String> {
    Ok(()) // Android 暂不支持本地 MCP
}

/// 保存本地 MCP 列表并热重启隧道桥接进程
#[tauri::command]
#[cfg(not(target_os = "android"))]
async fn set_local_mcps(
    app: AppHandle,
    mcps: serde_json::Value,
    state: State<'_, SharedTunnelState>,
) -> Result<(), String> {
    let store = app.store(LOCAL_MCP_STORE).map_err(|e| e.to_string())?;
    store.set(LOCAL_MCP_KEY, mcps);
    store.save().map_err(|e| e.to_string())?;

    // 如果隧道正在运行，重启它以加载新配置
    let is_running = state.lock().unwrap().child.is_some();
    if is_running {
        // 读取当前运行参数并重启
        // 通过 stop + start 实现热重载，需要前端重新调 start_tunnel
        stop_tunnel_inner(&state);
        log::info!("本地 MCP 配置已更新，隧道已停止，等待前端重启");
    }

    Ok(())
}

fn load_local_mcps(app: &AppHandle) -> serde_json::Value {
    app.store(LOCAL_MCP_STORE)
        .ok()
        .and_then(|store| store.get(LOCAL_MCP_KEY))
        .unwrap_or(serde_json::Value::Array(vec![]))
}

// ── 入口 ──────────────────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(not(target_os = "android"))]
    let tunnel_state: SharedTunnelState = Arc::new(Mutex::new(TunnelState { child: None }));
    #[cfg(target_os = "android")]
    let tunnel_state: SharedTunnelState = Arc::new(Mutex::new(TunnelState {}));

    let builder = tauri::Builder::default()
        .manage(tunnel_state)
        .plugin(tauri_plugin_store::Builder::default().build());

    #[cfg(not(target_os = "android"))]
    let builder = builder.plugin(tauri_plugin_shell::init());

    builder
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
        .invoke_handler(tauri::generate_handler![
            start_tunnel,
            stop_tunnel,
            get_local_mcps,
            set_local_mcps,
        ])
        .run(tauri::generate_context!())
        .expect("运行 Tauri 应用时出错");
}
