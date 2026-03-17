use ds_api::tool;
use sqlx::PgPool;
use std::path::PathBuf;
use uuid::Uuid;

/// SkillSpell 支持从数据库读取指定用户的 skill 内容（优先），
/// 若数据库中未找到或发生错误则回退到文件系统中的 `/srv/familiar/skills/*.md` 文件。
pub struct SkillSpell {
    /// 仍然保留文件系统目录以作回退。
    pub skills_dir: PathBuf,
    /// 可选的数据库连接池；如果为 `None`，则只使用文件系统。
    pub pool: Option<PgPool>,
    /// 可选的用户 id；若为 `None`，不会尝试从 DB 加载。
    pub user_id: Option<Uuid>,
}

#[tool]
impl Tool for SkillSpell {
    /// 加载指定 skill 的完整指令内容。
    /// 当你判断某个 skill 与当前任务相关时调用，获取详细操作规范。
    ///
    /// name: skill 名称（与可用 skill 列表中的 name 一致）
    async fn load_skill(&self, name: String) -> String {
        // 防止路径穿越或奇怪的名字
        if name.contains('/') || name.contains('.') {
            return "error: 无效的 skill 名称".into();
        }

        // 优先尝试从数据库读取（如果 pool 与 user_id 可用）
        if let (Some(pool), Some(user_id)) = (&self.pool, &self.user_id) {
            // 尝试按 user_id + name 查询最新的 content
            match sqlx::query_scalar::<_, String>(
                "SELECT content FROM user_skills WHERE user_id = $1 AND name = $2 ORDER BY created_at DESC LIMIT 1",
            )
            .bind(user_id)
            .bind(&name)
            .fetch_optional(pool)
            .await
            {
                Ok(Some(content)) => {
                    return strip_frontmatter(&content);
                }
                Ok(None) => {
                    // DB 中没找到 —— 继续回退到文件系统
                }
                Err(_) => {
                    // DB 查询失败 —— 继续回退到文件系统
                }
            }
        }

        // 文件系统回退：从 skills_dir 下读取 {name}.md
        let path = self.skills_dir.join(format!("{name}.md"));
        match tokio::fs::read_to_string(&path).await {
            Ok(content) => strip_frontmatter(&content),
            Err(_) => format!("error: skill '{name}' 不存在"),
        }
    }
}

/// 去掉 YAML frontmatter（--- ... ---），返回正文。
fn strip_frontmatter(content: &str) -> String {
    let content = content.trim_start();
    if !content.starts_with("---") {
        return content.to_string();
    }
    // 找第二个 ---
    if let Some(end) = content[3..].find("\n---") {
        let body_start = 3 + end + 4; // skip "\n---"
        content[body_start..].trim_start_matches('\n').to_string()
    } else {
        content.to_string()
    }
}
