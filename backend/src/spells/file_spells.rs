use ds_api::tool;
use serde_json::{Value, json};
use tokio::fs;

pub struct FileSpells;

#[tool]
impl Tool for FileSpells {
    /// 读取文件内容（行号 1-based）。
    /// 不传行范围时返回全文；文件超过 300 行时自动改为返回符号大纲，
    /// 模型再按需用 from/to 读取具体段落。
    ///
    /// description: 本次操作意图（供 UI 渲染，可不填）
    /// path: 文件路径
    /// from: 起始行（含），可选
    /// to: 结束行（含），可选
    async fn read(
        &self,
        description: Option<String>,
        path: String,
        from: Option<usize>,
        to: Option<usize>,
    ) -> Value {
        let _ = description;
        let content = match fs::read_to_string(&path).await {
            Ok(s) => s,
            Err(e) => return json!({ "error": e.to_string() }),
        };
        let lines: Vec<&str> = content.lines().collect();
        let total = lines.len();

        // 未指定行范围 + 文件大 → 在阻塞线程上做 tree-sitter 解析，避免卡住 async executor
        if from.is_none() && to.is_none() && total > super::OUTLINE_THRESHOLD {
            return tokio::task::spawn_blocking(move || {
                super::outline_value(&path, &content, total)
            })
            .await
            .unwrap_or_else(|_| serde_json::json!({ "error": "outline task panicked" }));
        }

        let from_1 = from.unwrap_or(1).max(1);
        let to_1 = to.unwrap_or(total).min(total);

        if from_1 > total {
            return json!({ "error": format!("from ({from_1}) 超出总行数 {total}") });
        }
        if from_1 > to_1 {
            return json!({ "error": format!("from ({from_1}) > to ({to_1})") });
        }

        let slice =
            super::truncate_output(&lines[from_1 - 1..to_1].join("\n"), super::MAX_OUTPUT_CHARS);
        json!({ "content": slice, "from": from_1, "to": to_1, "total_lines": total })
    }

    /// 写入文件（新建或完全覆盖）。
    /// 适合新建文件或大幅重写；小改动请用 edit 节省 token。
    ///
    /// description: 本次操作意图（供 UI 渲染，可不填）
    /// path: 文件路径（父目录不存在时自动创建）
    /// content: 写入的完整内容
    async fn write(&self, description: Option<String>, path: String, content: String) -> Value {
        let _ = description;
        if let Some(parent) = std::path::Path::new(&path).parent() {
            if !parent.as_os_str().is_empty() {
                if let Err(e) = fs::create_dir_all(parent).await {
                    return json!({ "error": format!("创建目录失败: {e}") });
                }
            }
        }
        match fs::write(&path, &content).await {
            Ok(_) => json!({ "status": "success", "lines_written": content.lines().count() }),
            Err(e) => json!({ "error": e.to_string() }),
        }
    }

    /// 精确替换文件中唯一的一处文本片段。
    /// old_str 须在文件中恰好出现一次，否则返回错误（0 次 = 找不到，多次 = 不唯一）。
    /// 成功后返回修改位置附近前后各 3 行上下文，便于确认结果。
    ///
    /// description: 本次操作意图（供 UI 渲染，可不填）
    /// path: 文件路径
    /// old_str: 要替换的原始文本（须与文件内容完全一致，含空格和换行）
    /// new_str: 替换后的新文本
    async fn edit(
        &self,
        description: Option<String>,
        path: String,
        old_str: String,
        new_str: String,
    ) -> Value {
        let _ = description;
        let content = match fs::read_to_string(&path).await {
            Ok(s) => s,
            Err(e) => return json!({ "error": e.to_string() }),
        };
        match content.matches(old_str.as_str()).count() {
            0 => {
                json!({ "error": "未找到匹配文本，请检查 old_str 是否与文件内容完全一致（含缩进和换行）" })
            }
            n if n > 1 => {
                json!({ "error": format!("找到 {n} 处匹配，old_str 不唯一，请加入更多上下文") })
            }
            _ => {
                let new_content = content.replacen(old_str.as_str(), &new_str, 1);
                if let Err(e) = fs::write(&path, &new_content).await {
                    return json!({ "error": e.to_string() });
                }
                let byte_off = new_content.find(new_str.as_str()).unwrap_or(0);
                let start_line = new_content[..byte_off].lines().count();
                let new_lines = new_str.lines().count().max(1);
                let all: Vec<&str> = new_content.lines().collect();
                const CTX: usize = 3;
                let a = start_line.saturating_sub(CTX);
                let b = (start_line + new_lines + CTX).min(all.len());
                let context: Vec<Value> = all[a..b]
                    .iter()
                    .enumerate()
                    .map(|(i, l)| json!({ "line": a + i + 1, "text": l }))
                    .collect();
                json!({ "status": "success", "context": context })
            }
        }
    }

    /// 删除文件。
    ///
    /// description: 本次操作意图（供 UI 渲染，可不填）
    /// path: 文件路径
    async fn delete(&self, description: Option<String>, path: String) -> Value {
        let _ = description;
        match fs::remove_file(&path).await {
            Ok(_) => json!({ "status": "success" }),
            Err(e) => json!({ "error": e.to_string() }),
        }
    }
}
