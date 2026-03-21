use agentix::tool;
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

pub struct PlanSpell {
    pub pool: PgPool,
    pub conversation_id: Uuid,
}

#[tool]
impl Tool for PlanSpell {
    /// 创建或更新执行计划，以结构化步骤列表的形式展示给用户。
    /// 应在开始复杂任务之前调用一次，之后随着任务推进随时调用来更新步骤状态。
    /// 前端会将计划渲染为可视化的任务列表。
    ///
    /// description: 本次操作意图（供 UI 渲染，可不填）
    /// title: 计划标题（简短说明要完成什么）
    /// steps_json: JSON 数组字符串，每项包含：
    ///   - id: 步骤唯一标识（字符串，如 "1"、"2a"）
    ///   - content: 步骤描述
    ///   - status: "pending" | "in_progress" | "completed" | "skipped"
    ///   - priority: "high" | "medium" | "low"（可选，默认 "medium"）
    ///
    /// 示例 steps_json:
    /// [{"id":"1","content":"分析需求","status":"completed","priority":"high"},
    ///  {"id":"2","content":"实现功能","status":"in_progress","priority":"high"},
    ///  {"id":"3","content":"编写测试","status":"pending","priority":"medium"}]
    async fn todo_list(
        &self,
        description: Option<String>,
        title: String,
        steps_json: String,
    ) -> serde_json::Value {
        let _ = description;
        let steps: serde_json::Value = serde_json::from_str(&steps_json)
            .unwrap_or(serde_json::Value::Array(vec![]));

        let _ = sqlx::query(
            r#"
            INSERT INTO conversation_plans (conversation_id, title, steps_json, updated_at)
            VALUES ($1, $2, $3, NOW())
            ON CONFLICT (conversation_id) DO UPDATE SET
                title      = EXCLUDED.title,
                steps_json = EXCLUDED.steps_json,
                updated_at = NOW()
            "#,
        )
        .bind(self.conversation_id)
        .bind(&title)
        .bind(&steps_json)
        .execute(&self.pool)
        .await;

        json!({
            "display": "plan",
            "title": title,
            "steps": steps,
        })
    }
}
