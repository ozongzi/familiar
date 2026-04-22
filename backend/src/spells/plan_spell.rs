use agentix::schemars::JsonSchema;
use agentix::tool;
use serde::Deserialize;
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

pub struct PlanSpell {
    pub pool: PgPool,
    pub conversation_id: Uuid,
}

#[derive(Deserialize, JsonSchema)]
struct PlanStep {
    id: String,
    content: String,
    status: String,
    priority: String,
}

#[tool]
impl Tool for PlanSpell {
    /// 创建或更新当前对话的结构化执行计划。
    /// 计划包含步骤列表，每个步骤有 id、content、status（pending/in_progress/completed）和 priority（high/medium/low）。
    /// todos: 步骤列表（JSON 数组）
    async fn todo_list(&self, todos: Vec<PlanStep>) -> Value {
        let steps = todos;

        let plan_json = match serde_json::to_value(steps
            .iter()
            .map(|s| json!({ "id": s.id, "content": s.content, "status": s.status, "priority": s.priority }))
            .collect::<Vec<_>>())
        {
            Ok(v) => v,
            Err(e) => return json!({ "error": format!("序列化失败: {e}") }),
        };

        let steps_str = plan_json.to_string();
        let result = sqlx::query(
            r#"
            INSERT INTO conversation_plans (conversation_id, steps_json)
            VALUES ($1, $2)
            ON CONFLICT (conversation_id)
            DO UPDATE SET steps_json = $2, updated_at = NOW()
            "#,
        )
        .bind(self.conversation_id)
        .bind(&steps_str)
        .execute(&self.pool)
        .await;

        match result {
            Ok(_) => json!({ "status": "ok", "steps": steps.len() }),
            Err(e) => json!({ "error": e.to_string() }),
        }
    }
}
