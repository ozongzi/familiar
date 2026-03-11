use ds_api::tool;
use serde_json::{Value, json};
use uuid::Uuid;

fn percent_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push_str(&format!("%{b:02X}"));
            }
        }
    }
    out
}

const ROLODEX_BASE: &str = "https://agentrolodex.com";

/// Fetch a URL with a shared reqwest client and return parsed JSON, or an error Value.
async fn get_json(url: &str) -> Result<Value, String> {
    reqwest::get(url)
        .await
        .map_err(|e| format!("请求失败: {e}"))?
        .json::<Value>()
        .await
        .map_err(|e| format!("解析响应失败: {e}"))
}

/// Given any URL belonging to an agent (root or a sub-path like /api/a2a),
/// walk up the path segments until we find a /.well-known/agent.json that
/// returns 200, then return (card_json, origin) where origin is the scheme+host
/// root. Returns an Err string if not found.
async fn fetch_agent_card(url: &str) -> Result<(Value, String), String> {
    // Parse out the origin (scheme + host, no path)
    let parsed = reqwest::Url::parse(url).map_err(|e| format!("无效 URL '{url}': {e}"))?;
    let origin = format!("{}://{}", parsed.scheme(), parsed.host_str().unwrap_or(""));
    if let Some(port) = parsed.port() {
        let origin = format!("{origin}:{port}");
        return fetch_agent_card_from_origin(url, &origin).await;
    }
    fetch_agent_card_from_origin(url, &origin).await
}

async fn fetch_agent_card_from_origin(url: &str, origin: &str) -> Result<(Value, String), String> {
    // Collect candidate roots to try: the full URL stripped back to origin, one
    // segment at a time.  e.g. for "https://host/api/a2a" we try:
    //   https://host/api/a2a
    //   https://host/api
    //   https://host
    let parsed = reqwest::Url::parse(url).map_err(|e| format!("无效 URL '{url}': {e}"))?;

    let mut segments: Vec<&str> = parsed
        .path_segments()
        .map(|s| s.collect())
        .unwrap_or_default();
    // Remove trailing empty segments (trailing slash)
    while segments.last() == Some(&"") {
        segments.pop();
    }

    // Build candidate list from most-specific to least-specific
    let mut candidates: Vec<String> = Vec::new();
    loop {
        let path = if segments.is_empty() {
            String::new()
        } else {
            format!("/{}", segments.join("/"))
        };
        candidates.push(format!("{origin}{path}"));
        if segments.is_empty() {
            break;
        }
        segments.pop();
    }

    for base in &candidates {
        let card_url = format!("{base}/.well-known/agent.json");
        let resp = match reqwest::get(&card_url).await {
            Ok(r) => r,
            Err(_) => continue,
        };
        if resp.status().is_success() {
            match resp.json::<Value>().await {
                Ok(card) => return Ok((card, base.clone())),
                Err(_) => continue,
            }
        }
    }

    Err(format!(
        "在 '{url}' 的各级路径下均未找到 /.well-known/agent.json（尝试了 {} 个候选地址）",
        candidates.len()
    ))
}

pub struct A2aSpell;

#[tool]
impl Tool for A2aSpell {
    /// 从 AgentRolodex 目录搜索 A2A agent 列表。
    /// 返回每个 agent 的名称、描述、URL、skills 和 tags。
    /// 可选参数：
    /// q: 关键词搜索（名称/描述）
    /// tag: 按 tag 筛选
    /// limit: 最多返回几条，默认 20
    async fn a2a_list(&self, q: Option<String>, tag: Option<String>, limit: Option<u32>) -> Value {
        let limit = limit.unwrap_or(20);
        let mut url = format!("{ROLODEX_BASE}/api/agents?limit={limit}");
        if let Some(q) = q {
            url.push_str(&format!("&q={}", percent_encode(&q)));
        }
        if let Some(tag) = tag {
            url.push_str(&format!("&tag={}", percent_encode(&tag)));
        }

        let agents = match get_json(&url).await {
            Ok(v) => v,
            Err(e) => return json!({ "error": e }),
        };

        let Some(arr) = agents.as_array() else {
            return json!({ "error": "意外的响应格式", "raw": agents });
        };

        let items: Vec<Value> = arr
            .iter()
            .map(|a| {
                let skills: Vec<Value> = a["skills"]
                    .as_array()
                    .unwrap_or(&vec![])
                    .iter()
                    .map(|s| {
                        json!({
                            "id": s["id"],
                            "name": s["name"],
                            "description": s.get("description").cloned().unwrap_or(Value::Null),
                        })
                    })
                    .collect();

                json!({
                    "name": a["name"],
                    "description": a["description"],
                    "url": a["url"],
                    "type": a["type"],
                    "tags": a["tags"],
                    "skills": skills,
                    "verified": a["verified"],
                    "platform": a["platform"],
                })
            })
            .collect();

        let count = items.len();
        json!({ "agents": items, "count": count })
    }

    /// 获取某个 A2A agent 的完整 Agent Card（从 /.well-known/agent.json 拉取）。
    /// 包含该 agent 对外声明的能力、skills、输入输出模式、认证方式等。
    /// agent_url 可以是 agent 根地址或其 RPC 端点（如 /api/a2a），
    /// 会自动沿路径向上查找 /.well-known/agent.json。
    /// agent_url: agent 的任意层级 URL（从 a2a_list 获取，或直接填 RPC 端点）
    async fn a2a_describe(&self, agent_url: String) -> Value {
        match fetch_agent_card(&agent_url).await {
            Ok((card, base)) => {
                // Annotate with the resolved base so the caller knows what to pass to a2a_call
                let mut out = card;
                out["_resolved_base"] = json!(base);
                out
            }
            Err(e) => json!({ "error": e }),
        }
    }

    /// 向某个 A2A agent 发送消息并等待回复。
    /// 先从 /.well-known/agent.json 读取 RPC 端点（url 字段），
    /// 再以 JSON-RPC 2.0 message/send 方法发送消息。
    /// 若 agent 返回异步 Task，会轮询 tasks/get 直到完成（最多 30 次，每次 2 秒）。
    /// agent_url 可以是 agent 根地址或其 RPC 端点，会自动向上查找 Agent Card。
    /// agent_url: agent 的任意层级 URL（从 a2a_list 获取，或直接填 RPC 端点）
    /// message: 发给该 agent 的消息内容
    async fn a2a_call(&self, agent_url: String, message: String) -> Value {
        // ── 1. 拉取 Agent Card，确定 RPC 端点 ────────────────────────────────
        let (card, base) = match fetch_agent_card(&agent_url).await {
            Ok(pair) => pair,
            Err(e) => return json!({ "error": e }),
        };

        // A2A spec: card.url 是 agent 根地址，RPC 端点通常就是根地址本身，
        // 有些 agent 会在 card 里的 interfaces[].url 指定别的端点。
        // 先找 interfaces，找不到就用 card.url，再找不到就用解析出的 base。
        let rpc_url = card["interfaces"]
            .as_array()
            .and_then(|arr| arr.first())
            .and_then(|i| i["url"].as_str())
            .or_else(|| card["url"].as_str())
            .unwrap_or(&base)
            .to_string();

        // ── 2. 构造 JSON-RPC 2.0 message/send 请求 ───────────────────────────
        let request_id = Uuid::new_v4().to_string();
        let message_id = Uuid::new_v4().to_string();

        let body = json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": "message/send",
            "params": {
                "message": {
                    "messageId": message_id,
                    "role": "user",
                    "parts": [{ "kind": "text", "text": message }]
                }
            }
        });

        let client = reqwest::Client::new();
        let resp = match client
            .post(&rpc_url)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => return json!({ "error": format!("发送消息失败: {e}") }),
        };

        let rpc_resp: Value = match resp.json().await {
            Ok(v) => v,
            Err(e) => return json!({ "error": format!("解析响应失败: {e}") }),
        };

        if let Some(err) = rpc_resp.get("error") {
            return json!({ "error": err });
        }

        let result = &rpc_resp["result"];

        // ── 3. 解析结果 ───────────────────────────────────────────────────────
        // SendMessageResponse can be a Message or a Task
        // Message: has "role" field
        // Task: has "id" and "status" fields
        if result.get("role").is_some() {
            // Direct Message response
            let text = extract_text_from_parts(&result["parts"]);
            return json!({ "reply": text });
        }

        if let Some(task_id) = result["id"].as_str() {
            // Task response — check if already complete
            if let Some(state) = result["status"]["state"].as_str()
                && matches!(state, "completed" | "failed" | "canceled") {
                    return task_result_to_value(result, task_id);
                }

            // ── 4. 轮询直到完成 ───────────────────────────────────────────────
            return poll_task(&client, &rpc_url, task_id, 30, 2000).await;
        }

        // Fallback: return raw result
        json!({ "result": result })
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Extract all text content from an A2A `parts` array.
fn extract_text_from_parts(parts: &Value) -> String {
    parts
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|p| {
                    // A2A RC1: { "kind": "text", "text": "..." }
                    // Some agents use: { "type": "text", "text": "..." }
                    p.get("text").and_then(|t| t.as_str())
                })
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

/// Convert a completed/failed Task JSON value to a return Value.
fn task_result_to_value(task: &Value, task_id: &str) -> Value {
    let state = task["status"]["state"].as_str().unwrap_or("unknown");

    if state == "failed" {
        let msg = task["status"]["message"]["parts"]
            .as_array()
            .and_then(|p| p.first())
            .and_then(|p| p["text"].as_str())
            .unwrap_or("task failed");
        return json!({ "error": msg, "task_id": task_id });
    }

    // Collect text from artifacts
    let texts: Vec<String> = task["artifacts"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .map(|a| extract_text_from_parts(&a["parts"]))
        .filter(|s| !s.is_empty())
        .collect();

    if texts.is_empty() {
        json!({
            "task_id": task_id,
            "status": state,
            "note": "任务完成但无文本输出",
        })
    } else {
        json!({
            "reply": texts.join("\n"),
            "task_id": task_id,
        })
    }
}

/// Poll tasks/get until the task reaches a terminal state.
async fn poll_task(
    client: &reqwest::Client,
    rpc_url: &str,
    task_id: &str,
    max_polls: u32,
    interval_ms: u64,
) -> Value {
    for _ in 0..max_polls {
        tokio::time::sleep(std::time::Duration::from_millis(interval_ms)).await;

        let body = json!({
            "jsonrpc": "2.0",
            "id": Uuid::new_v4().to_string(),
            "method": "tasks/get",
            "params": { "id": task_id }
        });

        let resp = match client
            .post(rpc_url)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => return json!({ "error": format!("轮询失败: {e}"), "task_id": task_id }),
        };

        let rpc_resp: Value = match resp.json().await {
            Ok(v) => v,
            Err(e) => {
                return json!({ "error": format!("解析轮询响应失败: {e}"), "task_id": task_id });
            }
        };

        if let Some(err) = rpc_resp.get("error") {
            return json!({ "error": err, "task_id": task_id });
        }

        let task = &rpc_resp["result"];
        let state = task["status"]["state"].as_str().unwrap_or("");

        if matches!(state, "completed" | "failed" | "canceled") {
            return task_result_to_value(task, task_id);
        }
    }

    json!({
        "error": format!("超过最大轮询次数 ({max_polls})，任务未完成"),
        "task_id": task_id,
    })
}
