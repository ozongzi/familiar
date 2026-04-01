use agentix::tool;
use reqwest::Client;
use serde::{Deserialize, Serialize};

/// Per-result truncation limits.
const MAX_CONTENT_CHARS: usize = 1000;
const MAX_RAW_CHARS: usize = 2000;
const MAX_ANSWER_CHARS: usize = 1000;

pub struct TavilySpell {
    pub api_key: String,
    pub http: Client,
}

// ── Tavily request / response types ──────────────────────────────────────────

#[derive(Serialize)]
struct SearchRequest<'a> {
    query: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    search_depth: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    topic: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_results: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    include_answer: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    include_raw_content: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    days: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    include_domains: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    exclude_domains: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct SearchResponse {
    #[serde(default)]
    answer: Option<String>,
    #[serde(default)]
    results: Vec<SearchResult>,
}

#[derive(Deserialize)]
struct SearchResult {
    title: String,
    url: String,
    #[serde(default)]
    content: String,
    #[serde(default)]
    raw_content: Option<String>,
    #[serde(default)]
    score: f64,
}

#[derive(Serialize)]
struct ExtractRequest<'a> {
    urls: Vec<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    include_images: Option<bool>,
}

#[derive(Deserialize)]
struct ExtractResponse {
    #[serde(default)]
    results: Vec<ExtractResult>,
}

#[derive(Deserialize)]
struct ExtractResult {
    url: String,
    #[serde(default)]
    raw_content: String,
}

// ── helpers ───────────────────────────────────────────────────────────────────

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    // Truncate at a char boundary.
    let mut idx = max;
    while !s.is_char_boundary(idx) {
        idx -= 1;
    }
    &s[..idx]
}

fn format_results(answer: Option<String>, results: Vec<SearchResult>) -> String {
    let mut out = String::new();

    if let Some(ans) = answer {
        let ans = truncate(&ans, MAX_ANSWER_CHARS);
        out.push_str(&format!("Answer: {ans}\n\n"));
    }

    for (i, r) in results.iter().enumerate() {
        let content = truncate(&r.content, MAX_CONTENT_CHARS);
        out.push_str(&format!(
            "[{}] {} (score: {:.2})\nURL: {}\n{}\n",
            i + 1,
            r.title,
            r.score,
            r.url,
            content,
        ));
        if let Some(raw) = &r.raw_content {
            let raw = truncate(raw, MAX_RAW_CHARS);
            out.push_str(&format!("Full content (truncated): {raw}\n"));
        }
        out.push('\n');
    }

    if out.is_empty() {
        "No results found.".to_string()
    } else {
        out.trim_end().to_string()
    }
}

// ── Tool implementation ───────────────────────────────────────────────────────

#[tool]
impl Tool for TavilySpell {
    /// 使用 Tavily 搜索互联网上的最新信息。
    /// 适合查找新闻、事实、当前事件或任何需要实时数据的查询。
    ///
    /// query: 搜索查询
    /// search_depth: "basic"（快速，默认）或 "advanced"（深度，更慢）
    /// topic: "general"（默认）或 "news"
    /// max_results: 返回结果数，默认 5，最多 10
    /// days: 仅返回最近 N 天的结果（仅 topic=news 时生效）
    /// include_domains: 限定搜索域名列表
    /// exclude_domains: 排除的域名列表
    async fn tavily_search(
        &self,
        query: String,
        search_depth: Option<String>,
        topic: Option<String>,
        max_results: Option<u32>,
        days: Option<u32>,
        include_domains: Option<Vec<String>>,
        exclude_domains: Option<Vec<String>>,
    ) -> String {
        let max_results = max_results.unwrap_or(5).min(10);
        let req = SearchRequest {
            query: &query,
            search_depth: search_depth.as_deref(),
            topic: topic.as_deref(),
            max_results: Some(max_results),
            include_answer: Some(true),
            include_raw_content: None,
            days,
            include_domains,
            exclude_domains,
        };

        let resp = self
            .http
            .post("https://api.tavily.com/search")
            .bearer_auth(&self.api_key)
            .json(&req)
            .send()
            .await;

        match resp {
            Err(e) => format!("error: {e}"),
            Ok(r) if !r.status().is_success() => {
                format!("error: HTTP {}", r.status())
            }
            Ok(r) => match r.json::<SearchResponse>().await {
                Err(e) => format!("error parsing response: {e}"),
                Ok(body) => format_results(body.answer, body.results),
            },
        }
    }

    /// 从指定 URL 提取完整网页内容。
    /// 适合在 tavily_search 找到相关页面后获取完整内容。
    ///
    /// urls: 要提取的 URL 列表，最多 5 个
    async fn tavily_extract(&self, urls: Vec<String>) -> String {
        let urls: Vec<String> = urls.into_iter().take(5).collect();
        let url_refs: Vec<&str> = urls.iter().map(String::as_str).collect();

        let req = ExtractRequest {
            urls: url_refs,
            include_images: None,
        };

        let resp = self
            .http
            .post("https://api.tavily.com/extract")
            .bearer_auth(&self.api_key)
            .json(&req)
            .send()
            .await;

        match resp {
            Err(e) => format!("error: {e}"),
            Ok(r) if !r.status().is_success() => {
                format!("error: HTTP {}", r.status())
            }
            Ok(r) => match r.json::<ExtractResponse>().await {
                Err(e) => format!("error parsing response: {e}"),
                Ok(body) => {
                    if body.results.is_empty() {
                        return "No content extracted.".to_string();
                    }
                    body.results
                        .into_iter()
                        .map(|r| {
                            let content = truncate(&r.raw_content, MAX_RAW_CHARS);
                            format!("URL: {}\n{}\n", r.url, content)
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                        .trim_end()
                        .to_string()
                }
            },
        }
    }

    /// 对一个主题进行多步深度研究，综合多个来源给出详细报告。
    /// 比 tavily_search 慢但更全面，适合需要综合分析的复杂问题。
    ///
    /// query: 研究主题
    /// max_results: 最多参考结果数，默认 5
    async fn tavily_research(&self, query: String, max_results: Option<u32>) -> String {
        let max_results = max_results.unwrap_or(5).min(10);
        let req = SearchRequest {
            query: &query,
            search_depth: Some("advanced"),
            topic: None,
            max_results: Some(max_results),
            include_answer: Some(true),
            include_raw_content: Some(true),
            days: None,
            include_domains: None,
            exclude_domains: None,
        };

        let resp = self
            .http
            .post("https://api.tavily.com/search")
            .bearer_auth(&self.api_key)
            .json(&req)
            .send()
            .await;

        match resp {
            Err(e) => format!("error: {e}"),
            Ok(r) if !r.status().is_success() => {
                format!("error: HTTP {}", r.status())
            }
            Ok(r) => match r.json::<SearchResponse>().await {
                Err(e) => format!("error parsing response: {e}"),
                Ok(body) => format_results(body.answer, body.results),
            },
        }
    }

    /// 抓取指定 URL 的所有可访问链接（站点地图）。
    ///
    /// url: 起始 URL
    /// max_depth: 爬取深度，默认 1
    async fn tavily_map(&self, url: String, max_depth: Option<u32>) -> String {
        let mut req = serde_json::json!({ "url": url });
        if let Some(d) = max_depth {
            req["max_depth"] = Value::from(d.min(3));
        }

        let resp = self
            .http
            .post("https://api.tavily.com/map")
            .bearer_auth(&self.api_key)
            .json(&req)
            .send()
            .await;

        match resp {
            Err(e) => format!("error: {e}"),
            Ok(r) if !r.status().is_success() => {
                format!("error: HTTP {}", r.status())
            }
            Ok(r) => match r.json::<Value>().await {
                Err(e) => format!("error parsing response: {e}"),
                Ok(body) => {
                    let urls: Vec<&str> = body
                        .get("urls")
                        .and_then(|v| v.as_array())
                        .map(|arr| arr.iter().take(50).filter_map(|v| v.as_str()).collect())
                        .unwrap_or_default();
                    if urls.is_empty() {
                        "No URLs found.".to_string()
                    } else {
                        urls.join("\n")
                    }
                }
            },
        }
    }

    /// 爬取一个网站的多个页面并返回内容。
    ///
    /// url: 起始 URL
    /// max_depth: 爬取深度，默认 1，最多 2
    /// max_pages: 最多爬取页面数，默认 5，最多 10
    async fn tavily_crawl(
        &self,
        url: String,
        max_depth: Option<u32>,
        max_pages: Option<u32>,
    ) -> String {
        let req = serde_json::json!({
            "url": url,
            "max_depth": max_depth.unwrap_or(1).min(2),
            "limit": max_pages.unwrap_or(5).min(10),
        });

        let resp = self
            .http
            .post("https://api.tavily.com/crawl")
            .bearer_auth(&self.api_key)
            .json(&req)
            .send()
            .await;

        match resp {
            Err(e) => format!("error: {e}"),
            Ok(r) if !r.status().is_success() => {
                format!("error: HTTP {}", r.status())
            }
            Ok(r) => match r.json::<Value>().await {
                Err(e) => format!("error parsing response: {e}"),
                Ok(body) => {
                    let results = body
                        .get("results")
                        .and_then(|v| v.as_array())
                        .cloned()
                        .unwrap_or_default();
                    if results.is_empty() {
                        return "No pages crawled.".to_string();
                    }
                    results
                        .iter()
                        .map(|r| {
                            let url = r.get("url").and_then(|v| v.as_str()).unwrap_or("");
                            let content =
                                r.get("raw_content").and_then(|v| v.as_str()).unwrap_or("");
                            let content = truncate(content, MAX_RAW_CHARS);
                            format!("URL: {url}\n{content}\n")
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                        .trim_end()
                        .to_string()
                }
            },
        }
    }
}
