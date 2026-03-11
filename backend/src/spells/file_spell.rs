use ds_api::tool;
use glob::glob;
use serde_json::json;
use tokio::fs;

use super::{MAX_OUTPUT_CHARS, truncate_output};

pub struct FileSpell;

#[tool]
impl Tool for FileSpell {
    /// 删除文件
    /// path: 文件路径
    async fn delete(&self, path: String) -> Value {
        match std::fs::remove_file(path) {
            Ok(_) => json!({ "status": "success" }),
            Err(e) => json!({ "error": e.to_string() }),
        }
    }

    /// 将整个文件内容替换为 content。适合新建文件或大幅重写。
    /// 注意：会覆盖文件全部内容，小改动请用 str_replace。
    /// path: 文件路径
    /// content: 写入的完整内容
    async fn write(&self, path: String, content: String) -> Value {
        if let Some(parent) = std::path::Path::new(&path).parent()
            && !parent.as_os_str().is_empty()
                && let Err(e) = std::fs::create_dir_all(parent) {
                    return json!({ "error": format!("创建目录失败: {e}") });
                }
        match std::fs::write(&path, &content) {
            Ok(_) => {
                let line_count = content.lines().count();
                json!({ "status": "success", "lines_written": line_count })
            }
            Err(e) => json!({ "error": e.to_string() }),
        }
    }

    /// 在文件中替换所有匹配的文本片段。
    /// path: 文件路径, 支持 glob 通配符
    /// old_str: 要替换的原始文本
    /// new_str: 替换后的新文本
    /// dry_run: 是否为模拟运行，不实际修改文件内容。
    async fn replace_all(
        &self,
        path: String,
        old_str: String,
        new_str: String,
        dry_run: bool,
    ) -> impl Serialize {
        let mut results = vec![];

        // 解析 glob
        for entry in glob(&path).expect("Invalid glob pattern") {
            match entry {
                Ok(path) => {
                    if path.is_file() {
                        // 读取文件
                        let content = fs::read_to_string(&path).await.unwrap_or_default();

                        // 替换文本
                        let replaced = content.replace(&old_str, &new_str);

                        // 判断是否有变化
                        if replaced != content {
                            if !dry_run {
                                // 写回文件
                                if let Err(e) = fs::write(&path, replaced.clone()).await {
                                    results.push(json!({"file": path.to_string_lossy(), "error": e.to_string()}));
                                    continue;
                                }
                            }

                            // 记录变化
                            results.push(json!({
                                "file": path.to_string_lossy(),
                                "changed": true,
                                "preview": &replaced[..replaced.len().min(100)] // 只预览前100字符
                            }));
                        } else {
                            results.push(json!({
                                "file": path.to_string_lossy(),
                                "changed": false
                            }));
                        }
                    }
                }
                Err(e) => results.push(json!({"error": e.to_string()})),
            }
        }
    }

    /// 在文件中精确替换一处文本片段。
    /// old_str 必须在文件中唯一出现；若匹配到多处，返回错误并提示扩大上下文。
    /// 适合局部小改动，比 patch 更直观、不需要计算行号。
    /// 成功后返回修改位置附近的上下文行（前后各 3 行），方便确认结果。
    /// path: 文件路径
    /// old_str: 要替换的原始文本（必须与文件内容完全一致，包括空格和换行）
    /// new_str: 替换后的新文本
    async fn str_replace(&self, path: String, old_str: String, new_str: String) -> Value {
        let content = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => return json!({ "error": e.to_string() }),
        };

        let count = content.matches(&old_str as &str).count();
        match count {
            0 => json!({
                "error": "未找到匹配的文本片段，请检查 old_str 是否与文件内容完全一致（包括缩进和换行）"
            }),
            1 => {
                let new_content = content.replacen(&old_str as &str, &new_str, 1);
                if let Err(e) = std::fs::write(&path, &new_content) {
                    return json!({ "error": e.to_string() });
                }

                // 找到修改位置，返回前后各 3 行作为上下文（1-based 行号）
                let byte_offset = new_content.find(&new_str as &str).unwrap_or(0);
                let before = &new_content[..byte_offset];
                let start_line = before.lines().count(); // 0-based，new_str 从此行开始
                let new_str_lines = new_str.lines().count().max(1);
                let all_lines: Vec<&str> = new_content.lines().collect();
                let total = all_lines.len();

                const CONTEXT: usize = 3;
                let ctx_start = start_line.saturating_sub(CONTEXT);
                let ctx_end = (start_line + new_str_lines + CONTEXT).min(total);

                let context_lines: Vec<serde_json::Value> = all_lines[ctx_start..ctx_end]
                    .iter()
                    .enumerate()
                    .map(|(i, line)| {
                        json!({
                            "line": ctx_start + i + 1, // 1-based
                            "content": line,
                        })
                    })
                    .collect();

                json!({
                    "status": "success",
                    "context": context_lines,
                })
            }
            n => json!({
                "error": format!("找到 {n} 处匹配，old_str 不唯一，请在 old_str 中包含更多上下文使其唯一")
            }),
        }
    }

    /// 获取文件内容（行号为 1-based）。
    /// 若不传 from/to 则返回全文（受 8000 字符限制截断）。
    /// 传入行范围时返回第 from 行到第 to 行（含两端，从 1 开始）。
    /// 对于大文件，建议先用 outline 获取符号表定位行号，或用 get_file_info 查看总行数后分段读取。
    /// path: 文件路径
    /// from: 起始行号（含，从 1 开始），可选，默认第 1 行
    /// to: 结束行号（含，从 1 开始），可选，默认最后一行
    async fn get(&self, path: String, from: Option<usize>, to: Option<usize>) -> Value {
        let content = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => return json!({ "error": e.to_string() }),
        };

        let lines: Vec<&str> = content.lines().collect();
        let total = lines.len();

        let from_1 = from.unwrap_or(1);
        let to_1 = to.unwrap_or(total);

        if from_1 == 0 {
            return json!({
                "error": format!("from 行号从 1 开始，不能为 0。该文件共 {total} 行，有效范围：1–{total}")
            });
        }
        if from_1 > to_1 {
            return json!({
                "error": format!("from ({from_1}) 不能大于 to ({to_1})。该文件共 {total} 行，有效范围：1–{total}")
            });
        }
        if from_1 > total {
            return json!({
                "error": format!("from ({from_1}) 超出文件总行数。该文件共 {total} 行，有效范围：1–{total}")
            });
        }
        if to_1 > total {
            return json!({
                "error": format!("to ({to_1}) 超出文件总行数。该文件共 {total} 行，有效范围：1–{total}。如需读到末尾，可省略 to 参数或传 {total}")
            });
        }

        // 转为 0-based 索引
        let slice = truncate_output(&lines[(from_1 - 1)..to_1].join("\n"), MAX_OUTPUT_CHARS);
        json!({
            "content": slice,
            "lines": { "from": from_1, "to": to_1, "total": total },
        })
    }

    /// PATCH 文件内容，将第 from 行到第 to 行（含两端，1-based）替换为 new_content。
    /// 若 from == to + 1（即 to < from 且相差 1），则在 from 行前插入内容而不删除任何行。
    /// 注意：需要精确的行号，容易出错，推荐优先使用 str_replace。
    /// path: 文件路径
    /// from: 起始行号（含，1-based）
    /// to: 结束行号（含，1-based）
    /// new_content: 替换内容
    async fn patch(&self, path: String, from: usize, to: usize, new_content: String) -> Value {
        if from == 0 {
            return json!({ "error": "from 行号从 1 开始，不能为 0" });
        }
        if from > to + 1 {
            return json!({ "error": format!("from ({from}) 不能大于 to ({to}) + 1") });
        }

        let file_content = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => return json!({ "error": e.to_string() }),
        };

        let mut lines: Vec<&str> = file_content.lines().collect();
        let total = lines.len();

        // 转为 0-based
        let l = from - 1;
        let r = to; // 0-based exclusive end = to (1-based inclusive)

        if r > total {
            return json!({ "error": format!("to ({to}) 超出文件总行数 ({total})") });
        }

        lines.splice(l..r, new_content.lines());
        let updated_content = lines.join("\n");

        match std::fs::write(&path, updated_content) {
            Ok(_) => json!({ "status": "success" }),
            Err(e) => json!({ "error": e.to_string() }),
        }
    }

    /// 获取文件基本信息（大小、总行数）。在读取大文件前建议先调用此方法。
    /// path: 文件路径
    async fn get_file_info(&self, path: String) -> Value {
        match std::fs::read_to_string(&path) {
            Ok(content) => {
                let line_count = content.lines().count();
                json!({ "size": content.len(), "lines_number": line_count })
            }
            Err(e) => json!({ "error": e.to_string() }),
        }
    }

    /// 列出目录内容，返回结构化文件列表。
    /// path: 目录路径
    async fn list_dir(&self, path: String) -> Value {
        let entries = match std::fs::read_dir(&path) {
            Ok(e) => e,
            Err(e) => return json!({ "error": e.to_string() }),
        };

        let mut files: Vec<serde_json::Value> = Vec::new();
        let mut dirs: Vec<serde_json::Value> = Vec::new();

        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let Ok(meta) = entry.metadata() else { continue };

            if meta.is_dir() {
                dirs.push(json!({ "name": name, "type": "dir" }));
            } else {
                files.push(json!({
                    "name": name,
                    "type": "file",
                    "size": meta.len(),
                }));
            }
        }

        // 目录在前，文件在后，各自按名称排序
        dirs.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
        files.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));

        let mut entries_out = dirs;
        entries_out.extend(files);

        json!({ "path": path, "entries": entries_out })
    }

    /// 读二进制文件，以 xxd 风格十六进制+ASCII 格式输出。
    /// path: 文件路径
    /// begin: 起始字节偏移量（含）
    /// end: 结束字节偏移量（不含）
    async fn read_binary(&self, path: String, begin: usize, end: usize) -> Value {
        let content = match std::fs::read(&path) {
            Ok(c) => c,
            Err(e) => return json!({ "error": e.to_string() }),
        };

        if begin > content.len() {
            return json!({ "error": format!("begin ({begin}) 超出文件大小 ({len})", len = content.len()) });
        }
        let end = end.min(content.len());
        let slice = &content[begin..end];

        let mut lines = Vec::new();
        for (i, chunk) in slice.chunks(16).enumerate() {
            let offset = begin + i * 16;
            let hex: String = chunk
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<Vec<_>>()
                .join(" ");
            let ascii: String = chunk
                .iter()
                .map(|&b| {
                    if b.is_ascii_graphic() || b == b' ' {
                        b as char
                    } else {
                        '.'
                    }
                })
                .collect();
            lines.push(format!("{offset:08x}  {hex:<47}  |{ascii}|"));
        }

        let output = truncate_output(&lines.join("\n"), MAX_OUTPUT_CHARS);
        json!({ "content": output, "bytes_read": slice.len() })
    }

    /// 获取二进制文件大小（字节数）
    /// path: 文件路径
    async fn get_binary_info(&self, path: String) -> Value {
        match std::fs::read(&path) {
            Ok(content) => json!({ "size": content.len() }),
            Err(e) => json!({ "error": e.to_string() }),
        }
    }

    /// 创建文件夹（含所有中间层级）
    /// path: 文件夹路径
    async fn create_dir_all(&self, path: String) -> Value {
        match std::fs::create_dir_all(&path) {
            Ok(()) => json!({ "status": "success" }),
            Err(e) => json!({ "error": e.to_string() }),
        }
    }
}
