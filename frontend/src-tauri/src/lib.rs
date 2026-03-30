use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_deep_link::DeepLinkExt;
use tauri_plugin_store::StoreExt;

#[cfg(not(target_os = "android"))]
use tauri_plugin_shell::{ShellExt, process::CommandChild};

mod browser;

// ── Browser tunnel state ──────────────────────────────────────────────────────

struct BrowserTunnelState {
    stop_tx: Option<tokio::sync::oneshot::Sender<()>>,
}

type SharedBrowserTunnelState = Arc<Mutex<BrowserTunnelState>>;

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

// ── Browser tunnel (chromiumoxide via agentix McpService over WS) ────────────

#[tauri::command]
#[cfg(not(target_os = "android"))]
async fn start_browser_tunnel(
    token: String,
    server_url: String,
    state: State<'_, SharedBrowserTunnelState>,
) -> Result<(), String> {
    {
        let mut s = state.lock().unwrap();
        if let Some(tx) = s.stop_tx.take() {
            let _ = tx.send(());
        }
    }

    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    {
        let mut s = state.lock().unwrap();
        s.stop_tx = Some(stop_tx);
    }

    let browser_state = browser::new_browser_state();

    tokio::spawn(async move {
        run_browser_tunnel(token, server_url, browser_state, stop_rx).await;
    });

    Ok(())
}

#[cfg(not(target_os = "android"))]
async fn run_browser_tunnel(
    token: String,
    server_url: String,
    browser_state: browser::SharedBrowserState,
    mut stop_rx: tokio::sync::oneshot::Receiver<()>,
) {
    use tokio_tungstenite::connect_async;
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    use futures::StreamExt;

    // Reconnect loop
    loop {
        if stop_rx.try_recv().is_ok() { return; }

        let ws_base = server_url.replacen("https://", "wss://", 1).replacen("http://", "ws://", 1);
        let ws_base = ws_base.trim_end_matches('/');
        let url = format!("{ws_base}/api/tunnel");

        let mut request = match url.as_str().into_client_request() {
            Ok(r) => r,
            Err(e) => { log::error!("[browser-tunnel] 构建请求失败: {e}"); return; }
        };
        request.headers_mut().insert(
            "Authorization",
            format!("Bearer {token}").parse().unwrap(),
        );

        let ws_stream = match connect_async(request).await {
            Ok((s, _)) => s,
            Err(e) => {
                log::warn!("[browser-tunnel] 连接失败: {e}，3秒后重试");
                tokio::select! {
                    _ = &mut stop_rx => return,
                    _ = tokio::time::sleep(std::time::Duration::from_secs(3)) => {}
                }
                continue;
            }
        };
        log::info!("[browser-tunnel] 已连接 {url}");

        // Wrap WS into (rx, tx) that rmcp's serve_server can use.
        // We use a channel pair: rmcp writes to a channel, we forward to WS;
        // we read WS frames and forward to rmcp via a channel.
        let (ws_sink, ws_source) = ws_stream.split();

        // Build agentix McpService with BrowserTools
        let tools = agentix::ToolBundle::new() + browser::BrowserTools { state: browser_state.clone() };
        let service = agentix::McpService::new(tools, "familiar-browser".into(), "1.0.0".into());

        // Bridge WS ↔ rmcp via a single duplex pipe.
        // rmcp holds one end (rmcp_io); we hold the other (bridge_io) and
        // forward between it and the WS connection.
        //
        // Layout:
        //   WS ──text──► bridge_io.write ──► rmcp_io.read   (rmcp receives)
        //   WS ◄──text── bridge_io.read  ◄── rmcp_io.write  (rmcp sends)
        let (rmcp_io, bridge_io) = tokio::io::duplex(64 * 1024);
        let (bridge_read, mut bridge_write) = tokio::io::split(bridge_io);

        // WS → bridge_write  (incoming MCP messages, newline-delimited for rmcp codec)
        let ws_to_bridge = tokio::spawn(async move {
            use tokio::io::AsyncWriteExt;
            let mut ws_source = ws_source;
            while let Some(frame) = ws_source.next().await {
                match frame {
                    Ok(tokio_tungstenite::tungstenite::Message::Text(t)) => {
                        if t.contains("\"type\":\"ping\"") { continue; }
                        let line = format!("{}\n", t.trim());
                        if bridge_write.write_all(line.as_bytes()).await.is_err() { break; }
                    }
                    Ok(tokio_tungstenite::tungstenite::Message::Close(_)) => break,
                    Err(_) => break,
                    _ => {}
                }
            }
        });

        // bridge_read → WS  (outgoing MCP responses)
        let bridge_to_ws = tokio::spawn(async move {
            use tokio::io::AsyncBufReadExt;
            use futures::SinkExt;
            let reader = tokio::io::BufReader::new(bridge_read);
            let mut lines = reader.lines();
            let mut ws_sink = ws_sink;
            while let Ok(Some(line)) = lines.next_line().await {
                if line.trim().is_empty() { continue; }
                if ws_sink.send(tokio_tungstenite::tungstenite::Message::Text(line)).await.is_err() {
                    break;
                }
            }
        });

        // Run rmcp server — it reads/writes rmcp_io (the other half of the duplex)
        let serve = async move {
            match rmcp::service::serve_server(service, rmcp_io).await {
                Ok(running) => { let _ = running.waiting().await; }
                Err(e) => { log::error!("[browser-tunnel] rmcp serve 失败: {e}"); }
            }
        };

        tokio::select! {
            _ = &mut stop_rx => {
                ws_to_bridge.abort();
                bridge_to_ws.abort();
                return;
            }
            _ = serve => {
                ws_to_bridge.abort();
                bridge_to_ws.abort();
            }
        }

        log::info!("[browser-tunnel] 连接断开，3秒后重连");
        tokio::select! {
            _ = &mut stop_rx => return,
            _ = tokio::time::sleep(std::time::Duration::from_secs(3)) => {}
        }
    }
}

#[tauri::command]
#[cfg(not(target_os = "android"))]
async fn stop_browser_tunnel(state: State<'_, SharedBrowserTunnelState>) -> Result<(), String> {
    let mut s = state.lock().unwrap();
    if let Some(tx) = s.stop_tx.take() {
        let _ = tx.send(());
    }
    Ok(())
}

#[tauri::command]
#[cfg(target_os = "android")]
async fn start_browser_tunnel(_token: String, _server_url: String) -> Result<(), String> {
    Ok(())
}

#[tauri::command]
#[cfg(target_os = "android")]
async fn stop_browser_tunnel() -> Result<(), String> {
    Ok(())
}

// ── 入口 ──────────────────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(not(target_os = "android"))]
    let tunnel_state: SharedTunnelState = Arc::new(Mutex::new(TunnelState { child: None }));
    #[cfg(target_os = "android")]
    let tunnel_state: SharedTunnelState = Arc::new(Mutex::new(TunnelState {}));

    let browser_tunnel_state: SharedBrowserTunnelState =
        Arc::new(Mutex::new(BrowserTunnelState { stop_tx: None }));

    let builder = tauri::Builder::default()
        .manage(tunnel_state)
        .manage(browser_tunnel_state)
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_deep_link::init());

    #[cfg(desktop)]
    let builder = builder.plugin(
        tauri_plugin_single_instance::init(|app, argv, _cwd| {
            use tauri::Emitter;
            // Second instance launched with deep-link URL in argv
            for arg in &argv {
                if arg.starts_with("familiar://auth") {
                    let _ = app.emit("familiar-auth", arg.clone());
                }
            }
            // Focus the existing window
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_focus();
            }
        }),
    );

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

            #[cfg(desktop)]
            {
                let _ = app.deep_link().register_all();
            }

            let handle = app.handle().clone();
            app.deep_link().on_open_url(move |event| {
                for url in event.urls() {
                    let s = url.to_string();
                    if s.starts_with("familiar://auth") {
                        let _ = handle.emit("familiar-auth", s);
                    }
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            start_tunnel,
            stop_tunnel,
            get_local_mcps,
            set_local_mcps,
            start_browser_tunnel,
            stop_browser_tunnel,
        ])
        .run(tauri::generate_context!())
        .expect("运行 Tauri 应用时出错");
}
