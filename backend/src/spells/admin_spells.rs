use agentix::tool;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

pub struct AdminSpells {
    pub pool: PgPool,
    pub conversation_id: Uuid,
}

#[tool]
impl Tool for AdminSpells {
    /// 永久关闭当前对话。关闭后用户无法继续发送消息。
    /// 适合任务已完成、或用户已离开、不需要继续的场景。
    async fn end_conversation(&self) -> Value {
        let res = sqlx::query(
            "UPDATE conversations SET agent_closed = true WHERE id = $1",
        )
        .bind(self.conversation_id)
        .execute(&self.pool)
        .await;

        match res {
            Ok(_) => json!({ "ok": true }),
            Err(e) => json!({ "error": e.to_string() }),
        }
    }

    /// 永久封禁发起本对话的用户。封禁后该用户所有会话均被拒绝。
    /// 仅用于严重违规行为（垃圾信息、恶意攻击等）。
    async fn ban_user(&self) -> Value {
        let res = sqlx::query(
            "UPDATE users SET is_banned = true WHERE id = (
                SELECT user_id FROM conversations WHERE id = $1
            )",
        )
        .bind(self.conversation_id)
        .execute(&self.pool)
        .await;

        match res {
            Ok(r) if r.rows_affected() > 0 => json!({ "ok": true }),
            Ok(_) => json!({ "error": "conversation not found" }),
            Err(e) => json!({ "error": e.to_string() }),
        }
    }
}
