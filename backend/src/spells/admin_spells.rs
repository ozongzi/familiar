use agentix::tool;
use serde_json::{Value, json};
use sqlx::{PgPool, Row};
use uuid::Uuid;

pub struct AdminSpells {
    pub pool: PgPool,
    pub conversation_id: Uuid,
}

#[tool]
impl Tool for AdminSpells {
    /// 永久关闭当前对话。关闭后用户无法继续发送消息。
    /// 适合任务已完成、或用户已离开、不需要继续的场景。
    ///
    /// reason: 关闭原因，记录到审计日志
    async fn end_conversation(&self, reason: String) -> Value {
        let res = sqlx::query(
            "UPDATE conversations SET agent_closed = true WHERE id = $1",
        )
        .bind(self.conversation_id)
        .execute(&self.pool)
        .await;

        match res {
            Ok(_) => {
                let _ = crate::audit::log_audit(
                    &self.pool,
                    None,
                    Some(self.conversation_id),
                    "agent.end_conversation",
                    Some(json!({ "reason": reason })),
                    None,
                ).await;
                json!({ "ok": true })
            }
            Err(e) => json!({ "error": e.to_string() }),
        }
    }

    /// 永久封禁发起本对话的用户。封禁后该用户所有会话均被拒绝。
    /// 仅用于严重违规行为（垃圾信息、恶意攻击等）。
    ///
    /// reason: 封禁原因，记录到审计日志
    async fn ban_user(&self, reason: String) -> Value {
        let row = sqlx::query(
            "UPDATE users SET is_banned = true
             WHERE id = (SELECT user_id FROM conversations WHERE id = $1)
             RETURNING id",
        )
        .bind(self.conversation_id)
        .fetch_optional(&self.pool)
        .await;

        match row {
            Ok(Some(r)) => {
                let user_id: uuid::Uuid = r.try_get("id").unwrap_or(uuid::Uuid::nil());
                let _ = crate::audit::log_audit(
                    &self.pool,
                    Some(user_id),
                    Some(self.conversation_id),
                    "agent.ban_user",
                    Some(json!({ "reason": reason })),
                    None,
                ).await;
                json!({ "ok": true })
            }
            Ok(None) => json!({ "error": "conversation not found" }),
            Err(e) => json!({ "error": e.to_string() }),
        }
    }
}
