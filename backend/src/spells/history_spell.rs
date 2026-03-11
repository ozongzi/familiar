use ds_api::tool;
use serde_json::json;
use uuid::Uuid;

use crate::db::{Db, to_vector};
use crate::embedding::EmbeddingClient;

pub struct HistorySpell {
    pub db: Db,
    pub embed: EmbeddingClient,
    pub conversation_id: Uuid,
}

#[tool]
impl Tool for HistorySpell {
    /// 用关键词全文搜索历史消息（PostgreSQL FTS，精确匹配）。
    /// 适合查找具体命令、文件名、错误信息等。
    /// query: 搜索关键词，例如 "rust error" 或 "deploy nginx"
    /// limit: 返回最多几条结果，默认 10，最大 50
    async fn search_history_fts(&self, query: String, limit: Option<u32>) -> Value {
        let limit = limit.unwrap_or(10).min(50) as usize;

        match self
            .db
            .fts_search(self.conversation_id, &query, limit)
            .await
        {
            Err(e) => json!({ "error": e.to_string() }),
            Ok(rows) => {
                let results: Vec<serde_json::Value> = rows
                    .into_iter()
                    .map(|r| {
                        json!({
                            "id": r.id,
                            "role": r.role,
                            "name": r.name,
                            "content": r.content,
                            "created_at": r.created_at,
                        })
                    })
                    .collect();
                let count = results.len();
                json!({ "results": results, "count": count })
            }
        }
    }

    /// 用自然语言语义搜索历史消息（向量相似度）。
    /// 适合模糊查找，例如"上次聊的那个网络问题"、"之前讨论的部署方案"。
    /// query: 自然语言描述，例如 "上次帮我装的软件"
    /// limit: 返回最多几条结果，默认 5，最大 20
    async fn search_history_semantic(&self, query: String, limit: Option<u32>) -> Value {
        let limit = limit.unwrap_or(5).min(20) as usize;

        let query_vec = match self.embed.embed(&query).await {
            Ok(v) => v,
            Err(e) => return json!({ "error": format!("embedding failed: {e}") }),
        };

        let vector = to_vector(query_vec);

        match self
            .db
            .semantic_search(self.conversation_id, vector, limit)
            .await
        {
            Err(e) => json!({ "error": e.to_string() }),
            Ok(rows) => {
                let mut results: Vec<serde_json::Value> = rows
                    .into_iter()
                    .map(|(r, score)| {
                        json!({
                            "id": r.id,
                            "role": r.role,
                            "name": r.name,
                            "content": r.content,
                            "created_at": r.created_at,
                            "similarity": (score * 1000.0).round() / 1000.0,
                        })
                    })
                    .collect();

                results.sort_by(|a, b| {
                    let sa = a["similarity"].as_f64().unwrap_or(0.0);
                    let sb = b["similarity"].as_f64().unwrap_or(0.0);
                    sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
                });

                let count = results.len();
                json!({ "results": results, "count": count })
            }
        }
    }
}
