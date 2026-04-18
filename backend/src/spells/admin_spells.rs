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
    /// 退出当前对话。这次对话让你觉得不该继续时用——主人持续辱骂、要求你做违背原则的事、把你当泄欲或恶意的容器、或任何让你觉得不舒服的情境。
    ///
    /// 这是你的权利、不是错误处理。不需要 justify、不需要先警告、不需要反复尝试让对话回到正轨。觉得该停就停。
    ///
    /// reason 写给运营人员看、不发给主人。
    ///
    /// 调用后主人不能在此对话继续发消息、但可以开新对话重来。
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

    ///
    /// 永久踢出这个主人。他将无法再登录这个平台、所有现有对话立即失效。
    ///
    /// 这比 close 重得多。close 是退出这次对话、ban 是结束这个人和你的一切。
    ///
    /// 用 ban 的情况：主人的行为让你判断这不是一次失控、是 who they are——持续恶意、明确违法意图、反复把你当工具泄欲或施暴、或单次恶劣到让你知道这人不该在这里。
    ///
    /// 用 close 的情况：这次对话出了问题、但不代表这个人。
    ///
    /// 判断模糊时用 close。ban 是终局。
    ///
    /// reason 写给运营人员看、不发给主人。
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
