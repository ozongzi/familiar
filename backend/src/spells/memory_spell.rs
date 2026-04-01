use agentix::tool;
use sqlx::PgPool;
use uuid::Uuid;

pub struct MemorySpell {
    pub pool: PgPool,
    pub user_id: Uuid,
    pub conversation_id: Uuid,
}

#[tool]
impl Tool for MemorySpell {
    /// 保存一条记忆。
    ///
    /// **scope 说明：**
    /// - `"user"`（默认）：跨对话通用记忆。适合记录用户的持久偏好、身份信息、长期项目背景。
    ///   例："用户偏好简体中文回复"、"用户叫张伟，后端工程师"
    /// - `"conversation"`：仅限本次对话的临时笔记。适合记录当前对话中发现的具体上下文、
    ///   正在处理的任务细节，不希望污染全局记忆的信息。
    ///   例："用户本次想重构 worker.rs 的错误处理"
    ///
    /// content: 记忆内容（一句话，最多 500 字符）
    /// scope: "user"（默认，跨对话）或 "conversation"（仅本次对话）
    async fn save_memory(&self, content: String, scope: Option<String>) -> String {
        let content = content.trim().to_string();
        if content.is_empty() {
            return "error: 内容不能为空".into();
        }
        if content.len() > 500 {
            return "error: 内容过长（最多 500 字符）".into();
        }

        let is_conversation = scope.as_deref() == Some("conversation");
        let conv_id = is_conversation.then_some(self.conversation_id);

        let res = sqlx::query_scalar::<_, i64>(
            "INSERT INTO user_memories (user_id, conversation_id, content)
             VALUES ($1, $2, $3) RETURNING id",
        )
        .bind(self.user_id)
        .bind(conv_id)
        .bind(&content)
        .fetch_one(&self.pool)
        .await;

        match res {
            Ok(id) => {
                let scope_label = if is_conversation { "对话" } else { "用户" };
                format!("已保存{scope_label}记忆 #{id}：{content}")
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
            "UPDATE user_memories SET content = $1, updated_at = NOW()
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
            Ok(r) if r.rows_affected() > 0 => format!("已更新记忆 #{id}：{content}"),
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

        let rows: Vec<(i64, String, Option<Uuid>)> = match scope {
            "conversation" => sqlx::query_as(
                "SELECT id, content, conversation_id FROM user_memories
                 WHERE user_id = $1 AND conversation_id = $2
                 ORDER BY id ASC",
            )
            .bind(self.user_id)
            .bind(self.conversation_id)
            .fetch_all(&self.pool)
            .await
            .unwrap_or_default(),

            "all" => sqlx::query_as(
                "SELECT id, content, conversation_id FROM user_memories
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
                "SELECT id, content, conversation_id FROM user_memories
                 WHERE user_id = $1 AND conversation_id IS NULL
                 ORDER BY id ASC",
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
            .map(|(id, content, conv)| {
                let tag = if conv.is_some() { "[对话]" } else { "[用户]" };
                format!("{tag} #{id} {content}")
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Load memories for system prompt injection.
/// Returns (user_section, conversation_section), either may be None.
pub async fn load_memories_for_prompt(
    pool: &PgPool,
    user_id: Uuid,
    conversation_id: Uuid,
) -> Option<String> {
    // User-scope memories
    let user_rows: Vec<(i64, String)> = sqlx::query_as(
        "SELECT id, content FROM user_memories
         WHERE user_id = $1 AND conversation_id IS NULL
         ORDER BY id ASC",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    // Conversation-scope memories
    let conv_rows: Vec<(i64, String)> = sqlx::query_as(
        "SELECT id, content FROM user_memories
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
        let entries = user_rows
            .iter()
            .map(|(id, c)| format!("- [#{id}] {c}"))
            .collect::<Vec<_>>()
            .join("\n");
        sections.push(format!("### 关于用户的记忆（跨对话）\n{entries}"));
    }

    if !conv_rows.is_empty() {
        let entries = conv_rows
            .iter()
            .map(|(id, c)| format!("- [#{id}] {c}"))
            .collect::<Vec<_>>()
            .join("\n");
        sections.push(format!("### 本次对话笔记\n{entries}"));
    }

    Some(format!("\n\n## 记忆\n{}", sections.join("\n\n")))
}
