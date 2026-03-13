use ds_api::tool;
use std::path::PathBuf;

pub struct SkillSpell {
    pub skills_dir: PathBuf,
}

#[tool]
impl Tool for SkillSpell {
    /// 加载指定 skill 的完整指令内容。
    /// 当你判断某个 skill 与当前任务相关时调用，获取详细操作规范。
    ///
    /// name: skill 名称（与可用 skill 列表中的 name 一致）
    async fn load_skill(&self, name: String) -> String {
        // 防止路径穿越
        if name.contains('/') || name.contains('.') {
            return "error: 无效的 skill 名称".into();
        }
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