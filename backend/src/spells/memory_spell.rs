use agentix::tool;
use sqlx::PgPool;
use uuid::Uuid;

use crate::embedding::EmbeddingClient;

pub struct MemorySpell {
    pub pool: PgPool,
    pub user_id: Uuid,
    pub conversation_id: Uuid,
    pub embed: EmbeddingClient,
}

#[tool]
impl Tool for MemorySpell {
    /// 保存一条记忆到长期记忆库。
    ///
    /// **写入前必须自问：未来的对话中，这条信息会改变我的行为吗？**
    /// 如果答案是否，则不要调用此工具。不要保存：
    /// - 一次性查询结果或临时状态
    /// - 常识性知识
    /// - 只在本次对话有效的上下文（用 scope="conversation" 代替）
    /// - 已经在系统提示或对话中显而易见的事实
    ///
    /// **category 说明：**
    /// - `"preference"`：用户的稳定偏好，未来对话中应默认遵守。
    ///   例："用户偏好简体中文回复"、"代码示例用 TypeScript"
    /// - `"procedure"`：高杠杆的操作流程，能节省未来大量探索时间。
    ///   例："该项目用 `just deploy` 部署"、"测试命令是 `cargo test -p codex-tui`"
    /// - `"fact"`：关于用户身份/环境/项目的持久事实。
    ///   例："用户叫张伟，后端工程师"、"主要项目在 ~/code/web/familiar"
    /// - `"note"`：其他值得跨对话保留的笔记（默认）
    ///
    /// **scope 说明：**
    /// - `"user"`（默认）：跨对话通用记忆
    /// - `"conversation"`：仅限本次对话的临时笔记
    ///
    /// content: 记忆内容（一句话，最多 500 字符）
    /// category: "preference" | "procedure" | "fact" | "note"（默认 "note"）
    /// scope: "user"（默认）或 "conversation"
    async fn save_memory(
        &self,
        content: String,
        category: Option<String>,
        scope: Option<String>,
    ) -> String {
        let content = content.trim().to_string();
        if content.is_empty() {
            return "error: 内容不能为空".into();
        }
        if content.len() > 500 {
            return "error: 内容过长（最多 500 字符）".into();
        }

        let category = match category.as_deref().unwrap_or("note") {
            "preference" => "preference",
            "procedure" => "procedure",
            "fact" => "fact",
            _ => "note",
        };

        let is_conversation = scope.as_deref() == Some("conversation");
        let conv_id = is_conversation.then_some(self.conversation_id);

        let res = sqlx::query_scalar::<_, i64>(
            "INSERT INTO user_memories (user_id, conversation_id, content, category)
             VALUES ($1, $2, $3, $4) RETURNING id",
        )
        .bind(self.user_id)
        .bind(conv_id)
        .bind(&content)
        .bind(category)
        .fetch_one(&self.pool)
        .await;

        match res {
            Ok(id) => {
                // Fire-and-forget embedding
                let embed = self.embed.clone();
                let pool = self.pool.clone();
                let text = content.clone();
                tokio::spawn(async move {
                    if let Ok(vec) = embed.embed(&text).await {
                        let vec_str = format!(
                            "[{}]",
                            vec.iter()
                                .map(|f| f.to_string())
                                .collect::<Vec<_>>()
                                .join(",")
                        );
                        let _ = sqlx::query(
                            "UPDATE user_memories SET embedding = $1::vector WHERE id = $2",
                        )
                        .bind(&vec_str)
                        .bind(id)
                        .execute(&pool)
                        .await;
                    }
                });

                let scope_label = if is_conversation { "对话" } else { "用户" };
                format!("已保存{scope_label}记忆 [{category}] #{id}：{content}")
            }
            Err(e) => format!("error: {e}"),
        }
    }

    /// 更新一条已有记忆的内容。
    ///
    /// id: 记忆 ID（从 list_memories 获取）
    /// content: 新的记忆内容
    async fn update_memory(&self, id: i64, content: String) -> String {
        let content = content.trim().to_string();
        if content.is_empty() {
            return "error: 内容不能为空".into();
        }
        if content.len() > 500 {
            return "error: 内容过长（最多 500 字符）".into();
        }

        let rows = sqlx::query(
            "UPDATE user_memories SET content = $1, updated_at = NOW(), embedding = NULL
             WHERE id = $2 AND user_id = $3
               AND (conversation_id IS NULL OR conversation_id = $4)",
        )
        .bind(&content)
        .bind(id)
        .bind(self.user_id)
        .bind(self.conversation_id)
        .execute(&self.pool)
        .await;

        match rows {
            Ok(r) if r.rows_affected() > 0 => {
                // Re-embed asynchronously
                let embed = self.embed.clone();
                let pool = self.pool.clone();
                let text = content.clone();
                tokio::spawn(async move {
                    if let Ok(vec) = embed.embed(&text).await {
                        let vec_str = format!(
                            "[{}]",
                            vec.iter()
                                .map(|f| f.to_string())
                                .collect::<Vec<_>>()
                                .join(",")
                        );
                        let _ = sqlx::query(
                            "UPDATE user_memories SET embedding = $1::vector WHERE id = $2",
                        )
                        .bind(&vec_str)
                        .bind(id)
                        .execute(&pool)
                        .await;
                    }
                });
                format!("已更新记忆 #{id}：{content}")
            }
            Ok(_) => format!("error: 记忆 #{id} 不存在"),
            Err(e) => format!("error: {e}"),
        }
    }

    /// 删除一条记忆。
    ///
    /// id: 记忆 ID（从 list_memories 获取）
    async fn delete_memory(&self, id: i64) -> String {
        let rows = sqlx::query(
            "DELETE FROM user_memories
             WHERE id = $1 AND user_id = $2
               AND (conversation_id IS NULL OR conversation_id = $3)",
        )
        .bind(id)
        .bind(self.user_id)
        .bind(self.conversation_id)
        .execute(&self.pool)
        .await;

        match rows {
            Ok(r) if r.rows_affected() > 0 => format!("已删除记忆 #{id}"),
            Ok(_) => format!("error: 记忆 #{id} 不存在"),
            Err(e) => format!("error: {e}"),
        }
    }

    /// 列出当前用户的记忆。
    ///
    /// scope: "user"（默认）列出跨对话记忆；"conversation" 列出本次对话记忆；"all" 列出全部
    async fn list_memories(&self, scope: Option<String>) -> String {
        let scope = scope.as_deref().unwrap_or("user");

        let rows: Vec<(i64, String, String, Option<Uuid>)> = match scope {
            "conversation" => sqlx::query_as(
                "SELECT id, content, category, conversation_id FROM user_memories
                 WHERE user_id = $1 AND conversation_id = $2
                 ORDER BY id ASC",
            )
            .bind(self.user_id)
            .bind(self.conversation_id)
            .fetch_all(&self.pool)
            .await
            .unwrap_or_default(),

            "all" => sqlx::query_as(
                "SELECT id, content, category, conversation_id FROM user_memories
                 WHERE user_id = $1
                   AND (conversation_id IS NULL OR conversation_id = $2)
                 ORDER BY conversation_id NULLS FIRST, id ASC",
            )
            .bind(self.user_id)
            .bind(self.conversation_id)
            .fetch_all(&self.pool)
            .await
            .unwrap_or_default(),

            _ => sqlx::query_as(
                "SELECT id, content, category, conversation_id FROM user_memories
                 WHERE user_id = $1 AND conversation_id IS NULL
                 ORDER BY category, id ASC",
            )
            .bind(self.user_id)
            .fetch_all(&self.pool)
            .await
            .unwrap_or_default(),
        };

        if rows.is_empty() {
            return "暂无记忆".into();
        }

        rows.iter()
            .map(|(id, content, cat, conv)| {
                let scope_tag = if conv.is_some() { "[对话]" } else { "[用户]" };
                format!("{scope_tag} [{cat}] #{id} {content}")
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

// ── Prompt injection ──────────────────────────────────────────────────────────

const SEMANTIC_TOP_K: i64 = 20;
const FALLBACK_LIMIT: i64 = 30;

/// Load memories for system prompt injection.
///
/// Strategy:
/// 1. If an embedding is available for the current conversation's last user
///    message, do semantic retrieval (cosine top-K) for user-scope memories.
/// 2. Otherwise fall back to recency order (most recently updated first).
/// 3. Always load all conversation-scope memories (there are few of them).
///
/// Memories are grouped by category and injected as a structured section.
pub async fn load_memories_for_prompt(
    pool: &PgPool,
    user_id: Uuid,
    conversation_id: Uuid,
) -> Option<String> {
    // ── User-scope memories ───────────────────────────────────────────────────
    // Try semantic retrieval using the conversation's own compact_summary or
    // the latest user message as the query vector.  We look for a pre-computed
    // embedding stored on the conversation row itself (written by the worker).
    // If none exists, fall back to a plain recency query.
    let query_vec: Option<String> = sqlx::query_scalar(
        "SELECT embedding::text FROM conversations WHERE id = $1 AND embedding IS NOT NULL",
    )
    .bind(conversation_id)
    .fetch_optional(pool)
    .await
    .unwrap_or(None);

    let user_rows: Vec<(i64, String, String)> = if let Some(vec) = query_vec {
        // Semantic: cosine-nearest user memories
        sqlx::query_as(
            "SELECT id, content, category FROM user_memories
             WHERE user_id = $1 AND conversation_id IS NULL AND embedding IS NOT NULL
             ORDER BY embedding <=> $2::vector
             LIMIT $3",
        )
        .bind(user_id)
        .bind(&vec)
        .bind(SEMANTIC_TOP_K)
        .fetch_all(pool)
        .await
        .unwrap_or_default()
    } else {
        // Fallback: most recently updated
        sqlx::query_as(
            "SELECT id, content, category FROM user_memories
             WHERE user_id = $1 AND conversation_id IS NULL
             ORDER BY updated_at DESC
             LIMIT $2",
        )
        .bind(user_id)
        .bind(FALLBACK_LIMIT)
        .fetch_all(pool)
        .await
        .unwrap_or_default()
    };

    // ── Conversation-scope memories ───────────────────────────────────────────
    let conv_rows: Vec<(i64, String, String)> = sqlx::query_as(
        "SELECT id, content, category FROM user_memories
         WHERE user_id = $1 AND conversation_id = $2
         ORDER BY id ASC",
    )
    .bind(user_id)
    .bind(conversation_id)
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    if user_rows.is_empty() && conv_rows.is_empty() {
        return None;
    }

    let mut sections = Vec::new();

    if !user_rows.is_empty() {
        // Group by category for readability
        let mut by_cat: std::collections::BTreeMap<&str, Vec<String>> =
            std::collections::BTreeMap::new();
        for (id, content, cat) in &user_rows {
            by_cat
                .entry(cat.as_str())
                .or_default()
                .push(format!("- [#{id}] {content}"));
        }
        let cat_order = ["preference", "procedure", "fact", "note"];
        let mut lines = Vec::new();
        for cat in cat_order {
            if let Some(entries) = by_cat.get(cat) {
                let label = match cat {
                    "preference" => "偏好",
                    "procedure" => "操作流程",
                    "fact" => "事实",
                    _ => "笔记",
                };
                lines.push(format!("**{label}**"));
                lines.extend(entries.clone());
            }
        }
        sections.push(format!("### 关于用户的记忆（跨对话）\n{}", lines.join("\n")));
    }

    if !conv_rows.is_empty() {
        let entries = conv_rows
            .iter()
            .map(|(id, c, cat)| format!("- [#{id}][{cat}] {c}"))
            .collect::<Vec<_>>()
            .join("\n");
        sections.push(format!("### 本次对话笔记\n{entries}"));
    }

    Some(format!("\n\n## 记忆\n{}", sections.join("\n\n")))
}

// ── Conversation consolidation ────────────────────────────────────────────────

/// Called during compact: promote high-value conversation-scope memories to
/// user-scope, discard low-value ones (category="note" with no indication of
/// future relevance stays as-is; preference/procedure/fact get promoted).
pub async fn consolidate_conversation_memories(
    pool: &PgPool,
    user_id: Uuid,
    conversation_id: Uuid,
) {
    // Promote preference / procedure / fact → user scope
    let _ = sqlx::query(
        "UPDATE user_memories
         SET conversation_id = NULL, updated_at = NOW()
         WHERE user_id = $1
           AND conversation_id = $2
           AND category IN ('preference', 'procedure', 'fact')",
    )
    .bind(user_id)
    .bind(conversation_id)
    .execute(pool)
    .await;

    // Delete remaining conversation-scope notes (ephemeral)
    let _ = sqlx::query(
        "DELETE FROM user_memories
         WHERE user_id = $1 AND conversation_id = $2 AND category = 'note'",
    )
    .bind(user_id)
    .bind(conversation_id)
    .execute(pool)
    .await;
}
