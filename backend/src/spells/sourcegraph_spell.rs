use agentix::tool;
use serde::Serialize;

#[derive(Serialize)]
struct CodeMatch {
    #[serde(rename = "type")]
    match_type: String,
    repo: String,
    path: String,
    url: String,
    preview: String,
}

#[derive(Serialize)]
struct SearchResponse {
    results: Vec<CodeMatch>,
    count: usize,
}

#[tool]
/// 在 Sourcegraph 上搜索代码、库文档和开源项目。
/// 适合查找特定函数的用法、库的实现细节、或者技术问题的参考代码。
/// 支持过滤器：lang:rust、repo:tokio-rs/tokio、file:*.md 等。
///
/// query: 搜索查询，例如 "lang:rust axum Router" 或 "repo:serde-rs/serde Serialize"
/// limit: 返回结果数量，默认 10，最多 30
async fn search_code(&self, query: String, limit: Option<u32>) -> SearchResponse {
    let limit = limit.unwrap_or(10).min(30);
    let client = reqwest::Client::new();

    let response = match client
        .get("https://sourcegraph.com/.api/search/stream")
        .header("Accept", "text/event-stream")
        .query(&[
            ("q", format!("{} count:{}", query, limit)),
            ("t", "literal".to_string()),
            ("display", limit.to_string()),
        ])
        .send()
        .await
        .and_then(|r| r.error_for_status())
    {
        Ok(r) => r,
        Err(_) => {
            return SearchResponse {
                results: vec![],
                count: 0,
            };
        }
    };

    let text = match response.text().await {
        Ok(t) => t,
        Err(_) => {
            return SearchResponse {
                results: vec![],
                count: 0,
            };
        }
    };

    let mut results: Vec<CodeMatch> = Vec::new();

    'outer: for chunk in text.split("\n\n") {
        let mut event_type = "";
        let mut data_str = "";

        for line in chunk.lines() {
            if let Some(e) = line.strip_prefix("event: ") {
                event_type = e;
            } else if let Some(d) = line.strip_prefix("data: ") {
                data_str = d;
            }
        }

        if event_type != "matches" || data_str.is_empty() {
            continue;
        }

        let Ok(matches) = serde_json::from_str::<Vec<serde_json::Value>>(data_str) else {
            continue;
        };

        for m in matches {
            let match_type = m
                .get("type")
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .to_string();
            let repo = m
                .get("repository")
                .and_then(|r| r.as_str())
                .unwrap_or("")
                .to_string();
            let path = m
                .get("path")
                .and_then(|p| p.as_str())
                .unwrap_or("")
                .to_string();
            let url = format!("https://sourcegraph.com/{}/-/blob/{}", repo, path);

            let preview = m
                .get("chunkMatches")
                .or_else(|| m.get("lineMatches"))
                .and_then(|c| c.as_array())
                .map(|arr| {
                    arr.iter()
                        .take(3)
                        .filter_map(|chunk| {
                            chunk
                                .get("content")
                                .and_then(|c| c.as_str())
                                .map(|s| s.trim().to_string())
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .unwrap_or_default();

            results.push(CodeMatch {
                match_type,
                repo,
                path,
                url,
                preview,
            });

            if results.len() >= limit as usize {
                break 'outer;
            }
        }
    }

    let count = results.len();
    SearchResponse { results, count }
}
