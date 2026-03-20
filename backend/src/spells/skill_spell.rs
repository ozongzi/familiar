use ds_api::tool;
use sqlx::PgPool;
use uuid::Uuid;

/// SkillSpell 支持先读取用户私有 skill，再回退到全局默认 skill（均存于数据库）。
pub struct SkillSpell {
    pub pool: PgPool,
    pub user_id: Uuid,
}

#[tool]
impl Tool for SkillSpell {
    /// 加载指定 skill 的完整指令内容。
    /// name: skill 名称
    async fn load_skill(&self, name: String) -> String {
        if name.contains('/') || name.contains('.') {
            return "error: 无效的 skill 名称".into();
        }

        let user_res = sqlx::query_scalar::<_, String>(
            "SELECT content FROM user_skills WHERE user_id = $1 AND name = $2 ORDER BY created_at DESC LIMIT 1",
        )
        .bind(self.user_id)
        .bind(&name)
        .fetch_optional(&self.pool)
        .await;

        if let Ok(Some(content)) = user_res {
            return strip_frontmatter(&content);
        }

        let app_res = sqlx::query_scalar::<_, String>(
            "SELECT content FROM app_skills WHERE name = $1 ORDER BY created_at DESC LIMIT 1",
        )
        .bind(&name)
        .fetch_optional(&self.pool)
        .await;

        match app_res {
            Ok(Some(content)) => strip_frontmatter(&content),
            _ => format!("error: skill '{name}' 不存在"),
        }
    }
}

fn strip_frontmatter(content: &str) -> String {
    let content = content.trim_start();
    if !content.starts_with("---") {
        return content.to_string();
    }
    if let Some(end) = content[3..].find("\n---") {
        let body_start = 3 + end + 4;
        content[body_start..].trim_start_matches('\n').to_string()
    } else {
        content.to_string()
    }
}
