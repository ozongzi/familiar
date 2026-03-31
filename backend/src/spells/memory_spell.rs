use agentix::tool;
use sqlx::PgPool;
use uuid::Uuid;

pub struct MemorySpell {
    pub pool: PgPool,
    pub user_id: Uuid,
}

#[tool]
impl Tool for MemorySpell {
    /// 保存一条新记忆。用于记录用户的重要偏好、事实或背景信息，供未来对话使用。
    /// 每条记忆应简洁、具体（一句话），例如：
    ///   "用户偏好使用简体中文回复"
    ///   "用户的名字叫张伟，职业是后端工程师"
    ///   "用户的工作项目是 familiar，技术栈是 Rust + React"
    ///
    /// content: 记忆内容（一句话描述）
    async fn save_memory(&self, content: String) -> String {
        let content = content.trim().to_string();
        if content.is_empty() {
            return "error: 内容不能为空".into();
        }
        if content.len() > 500 {
            return "error: 内容过长（最多 500 字符）".into();
        }

        let res = sqlx::query_scalar::<_, i64>(
            "INSERT INTO user_memories (user_id, content) VALUES ($1, $2) RETURNING id",
        )
        .bind(self.user_id)
        .bind(&content)
        .fetch_one(&self.pool)
        .await;

        match res {
            Ok(id) => format!("已保存记忆 #{id}：{content}"),
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
            "UPDATE user_memories SET content = $1, updated_at = NOW() WHERE id = $2 AND user_id = $3",
        )
        .bind(&content)
        .bind(id)
        .bind(self.user_id)
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
            "DELETE FROM user_memories WHERE id = $1 AND user_id = $2",
        )
        .bind(id)
        .bind(self.user_id)
        .execute(&self.pool)
        .await;

        match rows {
            Ok(r) if r.rows_affected() > 0 => format!("已删除记忆 #{id}"),
            Ok(_) => format!("error: 记忆 #{id} 不存在"),
            Err(e) => format!("error: {e}"),
        }
    }

    /// 列出当前用户的所有记忆。
    async fn list_memories(&self) -> String {
        let rows: Vec<(i64, String)> = sqlx::query_as(
            "SELECT id, content FROM user_memories WHERE user_id = $1 ORDER BY id ASC",
        )
        .bind(self.user_id)
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default();

        if rows.is_empty() {
            return "暂无记忆".into();
        }

        rows.iter()
            .map(|(id, content)| format!("[#{id}] {content}"))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Load all memories for a user, formatted for system prompt injection.
/// Returns None if the user has no memories.
pub async fn load_memories_for_prompt(pool: &PgPool, user_id: Uuid) -> Option<String> {
    let rows: Vec<(i64, String)> = sqlx::query_as(
        "SELECT id, content FROM user_memories WHERE user_id = $1 ORDER BY id ASC",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    if rows.is_empty() {
        return None;
    }

    let entries = rows
        .iter()
        .map(|(id, content)| format!("- [#{id}] {content}"))
        .collect::<Vec<_>>()
        .join("\n");

    Some(format!(
        "\n\n## 关于用户的记忆\n以下是你记住的关于用户的信息，请在回复时参考：\n{entries}"
    ))
}
