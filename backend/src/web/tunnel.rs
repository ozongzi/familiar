/// WebSocket 隧道 —— 客户端连接时，把 WS 连接直接作为 MCP transport，
/// 创建 McpTool 并注入到对应用户的 agent 中。
///
/// 协议：WS 上直接跑标准 MCP JSON-RPC 消息（rmcp 格式）。
/// 心跳：客户端发 {"type":"ping"}，服务器回 {"type":"pong"}（在 MCP 消息之外处理）。
use std::{collections::HashMap, sync::Arc};

use axum::{
    extract::{
        State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    response::IntoResponse,
};
use ds_api::{McpTool, Tool as _};
use futures::{Sink, SinkExt, Stream, StreamExt};
use rmcp::{
    service::{RoleClient, RxJsonRpcMessage, TxJsonRpcMessage},
    transport::sink_stream::SinkStreamTransport,
};
use serde_json::Value;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::{
    errors::AppError,
    web::{AppState, auth::AuthUser},
};

// ── 注册表 ────────────────────────────────────────────────────────────────────

/// 记录每个用户当前是否有客户端在线及其暴露的 MCP 工具。
pub type TunnelRegistry = Arc<Mutex<HashMap<Uuid, McpTool>>>;

pub fn new_tunnel_registry() -> TunnelRegistry {
    Arc::new(Mutex::new(HashMap::new()))
}

// ── WS ↔ MCP 适配层 ───────────────────────────────────────────────────────────

/// 把 axum WS sink 包成 rmcp 能用的 Sink<TxJsonRpcMessage>。
struct WsSink {
    inner: futures::stream::SplitSink<WebSocket, Message>,
}

impl Sink<TxJsonRpcMessage<RoleClient>> for WsSink {
    type Error = axum::Error;

    fn poll_ready(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.inner.poll_ready_unpin(cx)
    }

    fn start_send(
        mut self: std::pin::Pin<&mut Self>,
        item: TxJsonRpcMessage<RoleClient>,
    ) -> Result<(), Self::Error> {
        let text = serde_json::to_string(&item).map_err(|e| {
            axum::Error::new(std::io::Error::new(std::io::ErrorKind::InvalidData, e))
        })?;
        self.inner.start_send_unpin(Message::Text(text.into()))
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.inner.poll_flush_unpin(cx)
    }

    fn poll_close(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.inner.poll_close_unpin(cx)
    }
}

/// 把 axum WS stream 包成 rmcp 能用的 Stream<Item = RxJsonRpcMessage>。
/// 非 MCP 消息（ping、close 等）在这里过滤掉。
struct WsStream {
    inner: futures::stream::SplitStream<WebSocket>,
    /// 发 pong 用的 channel
    pong_tx: tokio::sync::mpsc::UnboundedSender<String>,
}

impl Stream for WsStream {
    type Item = RxJsonRpcMessage<RoleClient>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        loop {
            match self.inner.poll_next_unpin(cx) {
                std::task::Poll::Ready(Some(Ok(Message::Text(text)))) => {
                    // 先检查是不是 ping
                    if let Ok(v) = serde_json::from_str::<Value>(&text)
                        && v.get("type").and_then(|t| t.as_str()) == Some("ping")
                    {
                        let _ = self.pong_tx.send("{\"type\":\"pong\"}".to_string());
                        continue; // 继续读下一条
                    }
                    // 尝试解析成 MCP 消息
                    match serde_json::from_str::<RxJsonRpcMessage<RoleClient>>(&text) {
                        Ok(msg) => return std::task::Poll::Ready(Some(msg)),
                        Err(e) => {
                            tracing::warn!("无法解析 MCP 消息: {e}, raw: {text}");
                            continue;
                        }
                    }
                }
                std::task::Poll::Ready(Some(Ok(_))) => continue, // binary/ping/pong/close frame 忽略
                std::task::Poll::Ready(Some(Err(e))) => {
                    tracing::warn!("WS 读取错误: {e}");
                    return std::task::Poll::Ready(None);
                }
                std::task::Poll::Ready(None) => return std::task::Poll::Ready(None),
                std::task::Poll::Pending => return std::task::Poll::Pending,
            }
        }
    }
}

// ── HTTP 升级入口 ─────────────────────────────────────────────────────────────

pub async fn tunnel_handler(
    ws: WebSocketUpgrade,
    auth: AuthUser,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    let user_id = auth.user_id;
    Ok(ws.on_upgrade(move |socket| handle_tunnel(socket, user_id, state)))
}

// ── WS 主循环 ─────────────────────────────────────────────────────────────────

async fn handle_tunnel(socket: WebSocket, user_id: Uuid, state: AppState) {
    let (ws_tx, ws_rx) = socket.split();

    // pong channel：WsStream 检测到 ping 时往这里发，writer task 负责回复
    let (pong_tx, mut pong_rx) = tokio::sync::mpsc::unbounded_channel::<String>();

    // 启动 pong writer task
    // 我们需要共享 ws_tx，用 Arc<Mutex> 包一下
    let ws_sink = Arc::new(Mutex::new(WsSink { inner: ws_tx }));
    let ws_sink_for_pong = ws_sink.clone();
    let pong_task = tokio::spawn(async move {
        while let Some(msg) = pong_rx.recv().await {
            let mut sink = ws_sink_for_pong.lock().await;
            let _ = sink.inner.send(Message::Text(msg.into())).await;
        }
    });

    let ws_stream = WsStream {
        inner: ws_rx,
        pong_tx,
    };

    // 构建 MCP transport：需要一个不被 Arc 包裹的 sink
    // 用一个 channel 把 Arc<Mutex<WsSink>> 桥接成普通 Sink
    let (mcp_sink_tx, mut mcp_sink_rx) =
        tokio::sync::mpsc::unbounded_channel::<TxJsonRpcMessage<RoleClient>>();

    let ws_sink_for_mcp = ws_sink.clone();
    let mcp_writer_task = tokio::spawn(async move {
        while let Some(msg) = mcp_sink_rx.recv().await {
            let text = match serde_json::to_string(&msg) {
                Ok(t) => t,
                Err(e) => {
                    tracing::warn!("MCP 序列化失败: {e}");
                    continue;
                }
            };
            let mut sink = ws_sink_for_mcp.lock().await;
            let _ = sink.inner.send(Message::Text(text.into())).await;
        }
    });

    // channel sink 实现
    struct ChannelSink(tokio::sync::mpsc::UnboundedSender<TxJsonRpcMessage<RoleClient>>);
    impl Sink<TxJsonRpcMessage<RoleClient>> for ChannelSink {
        type Error = tokio::sync::mpsc::error::SendError<TxJsonRpcMessage<RoleClient>>;
        fn poll_ready(
            self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Result<(), Self::Error>> {
            std::task::Poll::Ready(Ok(()))
        }
        fn start_send(
            self: std::pin::Pin<&mut Self>,
            item: TxJsonRpcMessage<RoleClient>,
        ) -> Result<(), Self::Error> {
            self.0.send(item)
        }
        fn poll_flush(
            self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Result<(), Self::Error>> {
            std::task::Poll::Ready(Ok(()))
        }
        fn poll_close(
            self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Result<(), Self::Error>> {
            std::task::Poll::Ready(Ok(()))
        }
    }

    let channel_sink = ChannelSink(mcp_sink_tx);
    let transport = SinkStreamTransport::new(channel_sink, ws_stream);

    // 连接 MCP，获取工具列表
    let mcp_tool = match McpTool::from_transport(transport).await {
        Ok(t) => t.with_max_output_chars(8000), // 防止 snapshot 过大导致 WS 传输截断
        Err(e) => {
            tracing::warn!(%user_id, "客户端 MCP 握手失败: {e}");
            pong_task.abort();
            mcp_writer_task.abort();
            return;
        }
    };

    tracing::info!(%user_id, tools = mcp_tool.raw_tools().len(), "客户端隧道已连接");

    // 把隧道工具存进注册表，供 build_agent 时查询
    // 同时如果当前有活跃的 chat entry，也通过 inject_tx 实时注入
    {
        let mut registry = state.tunnel_registry.lock().await;
        registry.insert(user_id, mcp_tool.clone());
        tracing::info!(%user_id, "隧道工具已存入注册表");
    }

    // 如果当前有活跃的 chat entry，立即注入工具
    let injected = {
        let chats = state.chats.lock().unwrap();
        if let Some(entry) = chats.get(&user_id) {
            let _ = entry
                .tool_inject_tx
                .send(ds_api::ToolInjection::Add(Box::new(mcp_tool.clone())));
            true
        } else {
            false
        }
    };

    if injected {
        tracing::info!(%user_id, "已将隧道工具实时注入当前 agent");
    } else {
        tracing::info!(%user_id, "无活跃 agent，隧道工具将在下次对话时从注册表加载");
    }

    // 等待 writer tasks 结束（连接断开时自动结束）
    let _ = tokio::join!(pong_task, mcp_writer_task);

    // 断开时从注册表移除
    {
        let mut registry = state.tunnel_registry.lock().await;
        registry.remove(&user_id);
    }

    // 如果当前有活跃的 chat entry，通过 inject_tx 移除该用户的隧道工具
    {
        let tool_names: Vec<String> = mcp_tool
            .raw_tools()
            .iter()
            .map(|t| t.function.name.clone())
            .collect();
        let chats = state.chats.lock().unwrap();
        if let Some(entry) = chats.get(&user_id) {
            let _ = entry
                .tool_inject_tx
                .send(ds_api::ToolInjection::Remove(tool_names));
        }
    }

    tracing::info!(%user_id, "客户端隧道已断开");
}
