use std::time::Duration;

use ds_api::tool;
use glob::glob as glob_walk;
use serde_json::{Value, json};
use tokio::process::Command;
use crate::spells::count_lines;

pub struct SearchSpells;

#[tool]
impl Tool for SearchSpells {
    /// 用 ripgrep 在目录或文件中搜索匹配正则的内容，返回匹配行及上下文。
    /// 适合查找函数定义、变量引用、错误信息等。
    ///
    /// description: 本次操作意图（供 UI 渲染，可不填）
    /// pattern: 搜索正则（如 "fn build_agent"）
    /// path: 搜索范围，目录或文件（默认 "."）
    /// file_glob: 只搜此 glob 匹配的文件（如 "*.rs"），可选
    /// context_lines: 每个匹配前后各显示几行（默认 2，最多 10）
    /// case_sensitive: 大小写敏感（默认 false）
    /// literal: 精确字符串匹配而非正则（默认 false）
    async fn grep(
        &self,
        description: Option<String>,
        pattern: String,
        path: Option<String>,
        file_glob: Option<String>,
        context_lines: Option<u32>,
        case_sensitive: Option<bool>,
        literal: Option<bool>,
    ) -> Value {
        let _ = description;
        let search_path = path.unwrap_or_else(|| ".".into());
        let context = context_lines.unwrap_or(2).min(10);

        let mut cmd = Command::new("rg");
        cmd.args(["--json", "--max-count", "200"]);
        cmd.args(["--context", &context.to_string()]);
        if literal.unwrap_or(false) {
            cmd.arg("--fixed-strings");
        }
        if !case_sensitive.unwrap_or(false) {
            cmd.arg("--ignore-case");
        }
        if let Some(g) = file_glob {
            cmd.args(["--glob", &g]);
        }
        cmd.arg(&pattern).arg(&search_path);

        let output = match tokio::time::timeout(Duration::from_secs(20), cmd.output()).await {
            Err(_) => return json!({ "error": "搜索超时" }),
            Ok(Err(e)) => return json!({ "error": e.to_string() }),
            Ok(Ok(o)) => o,
        };
        if output.status.code() == Some(2) {
            return json!({ "error": String::from_utf8_lossy(&output.stderr).trim().to_string() });
        }
        if output.status.code() == Some(1) {
            return json!({ "matches": [], "message": "无匹配结果" });
        }

        let mut matches: Vec<Value> = vec![];
        let mut cur_file: Option<String> = None;
        let mut cur_lines: Vec<Value> = vec![];

        for raw in String::from_utf8_lossy(&output.stdout).lines() {
            let Ok(obj) = serde_json::from_str::<Value>(raw) else {
                continue;
            };
            match obj["type"].as_str() {
                Some("begin") => {
                    cur_file = obj["data"]["path"]["text"].as_str().map(String::from);
                    cur_lines.clear();
                }
                Some("match") | Some("context") => {
                    let is_match = obj["type"].as_str() == Some("match");
                    let line = obj["data"]["line_number"].as_u64().unwrap_or(0);
                    let text = obj["data"]["lines"]["text"]
                        .as_str()
                        .unwrap_or("")
                        .trim_end_matches('\n');
                    cur_lines.push(json!({ "line": line, "text": text, "is_match": is_match }));
                }
                Some("end") => {
                    if let Some(file) = cur_file.take()
                        && !cur_lines.is_empty() {
                            matches.push(json!({ "file": file, "lines": cur_lines.clone() }));
                        }
                    cur_lines.clear();
                }
                _ => {}
            }
        }

        json!({ "matches": matches, "file_count": matches.len() })
    }

    /// 按 glob 模式查找文件，返回匹配路径列表。
    /// 适合"找所有 *.rs"或"列出 src 下所有组件"。
    ///
    /// description: 本次操作意图（供 UI 渲染，可不填）
    /// pattern: glob 模式（如 "**/*.rs" 或 "src/**/*.tsx"）
    async fn glob(&self, description: Option<String>, pattern: String) -> Value {
        let _ = description;
        let mut paths: Vec<String> = vec![];
        let mut errors: Vec<String> = vec![];
        for entry in glob_walk(&pattern).unwrap_or_else(|_| glob_walk("").unwrap()) {
            match entry {
                Ok(p) => paths.push(p.to_string_lossy().into()),
                Err(e) => errors.push(e.to_string()),
            }
        }
        paths.sort();
        let mut out = json!({ "paths": paths, "count": paths.len() });
        if !errors.is_empty() {
            out["errors"] = json!(errors);
        }
        out
    }

    /// 提取源文件的符号大纲（函数、类、结构体等），返回名称、类型和行号范围。
    /// 适合在读大文件前快速了解结构，再用 read(from, to) 精确读取目标段落。
    /// 支持：Rust、Python、JS、TS/TSX、Go、Java、C、C++、TOML
    ///
    /// description: 本次操作意图（供 UI 渲染，可不填）
    /// path: 源文件路径
    async fn outline(&self, description: Option<String>, path: String) -> String {
        let _ = description;
        let total = count_lines(&path).await;
        outline_value(&path, total).await
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tree-sitter outline helpers (shared with file_spells via super::outline_value)
// ─────────────────────────────────────────────────────────────────────────────
pub(crate) async fn outline_value(path: &str, total_lines: usize) -> String {
    let output = match tokio::time::timeout(
        Duration::from_secs(10),
        Command::new("ctags")
            .args(["--fields=+ne", "--extras=-F", "-f", "-", path])
            .output(),
    )
        .await
    {
        Err(_) => return "error: ctags 超时".into(),
        Ok(Err(e)) => return format!("error: ctags 启动失败: {e}"),
        Ok(Ok(o)) => o,
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stderr.is_empty() {
            return format!("error: {stderr}");
        }
    }

    let mut symbols: Vec<(u64, String)> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|l| !l.starts_with('!'))
        .filter_map(|line| {
            let fields: Vec<&str> = line.split('\t').collect();
            if fields.len() < 4 { return None; }
            let name = fields[0];
            let kind = fields[3];
            let mut start = 0u64;
            let mut end = 0u64;
            for f in &fields[4..] {
                if let Some(v) = f.strip_prefix("line:") { start = v.parse().unwrap_or(0); }
                if let Some(v) = f.strip_prefix("end:")  { end   = v.parse().unwrap_or(0); }
            }
            if start == 0 { return None; }
            if end == 0 { end = start; }
            Some((start, format!("{kind} {name} ({start}-{end})")))
        })
        .collect();

    if symbols.is_empty() {
        return format!("{path} ({total_lines} lines) — ctags 未提取到符号，请用 read(from, to) 按段读取");
    }

    symbols.sort_by_key(|(line, _)| *line);

    let body = symbols.into_iter().map(|(_, s)| s).collect::<Vec<_>>().join("\n");
    format!("{path} ({total_lines} lines)\n{body}")
}