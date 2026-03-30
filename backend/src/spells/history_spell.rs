use agentix::tool;
use serde_json::json;
use uuid::Uuid;

use crate::db::Db;
use crate::embedding::EmbeddingClient;

pub struct HistorySpell {
    pub db: Db,
    pub embedding: EmbeddingClient,
    pub conversation_id: Uuid,
}

#[tool]
impl Tool for HistorySpell {
    /// 在对话历史中进行全文搜索。
    /// query: 搜索关键词
    /// limit: 最多返回条数，默认 10
    async fn history_fts(&self, query: String, limit: Option<usize>) -> Value {
        let limit = limit.unwrap_or(10);
        match self.db.fts_search(self.conversation_id, &query, limit).await {
            Ok(rows) => {
                let items: Vec<Value> = rows
                    .into_iter()
                    .map(|r| json!({
                        "role": r.role,
                        "content": r.content,
                        "created_at": r.created_at,
                    }))
                    .collect();
                json!({ "results": items })
            }
            Err(e) => json!({ "error": e.to_string() }),
        }
    }

    /// 在对话历史中进行语义搜索（向量相似度）。
    /// query: 查询文本
    /// limit: 最多返回条数，默认 5
    async fn history_semantic(&self, query: String, limit: Option<usize>) -> Value {
        let limit = limit.unwrap_or(5);
        let vec = match self.embedding.embed(&query).await {
            Ok(v) => v,
            Err(e) => return json!({ "error": format!("embed failed: {e}") }),
        };
        let query_vec = crate::db::to_vector(vec);
        match self.db.semantic_search(self.conversation_id, query_vec, limit).await {
            Ok(rows) => {
                let items: Vec<Value> = rows
                    .into_iter()
                    .map(|(r, score)| json!({
                        "role": r.role,
                        "content": r.content,
                        "score": score,
                        "created_at": r.created_at,
                    }))
                    .collect();
                json!({ "results": items })
            }
            Err(e) => json!({ "error": e.to_string() }),
        }
    }
}
