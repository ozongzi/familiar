use std::time::Duration;

use ds_api::tool;
use serde_json::{Value, json};
use tokio::process::Command;

use super::{MAX_OUTPUT_CHARS, truncate_output};

pub struct SearchSpell;

#[tool]
impl Tool for SearchSpell {
    /// 用 ripgrep 在目录或文件中搜索匹配正则的内容。
    /// 返回每个匹配的文件路径、行号、行内容，以及前后各 context_lines 行上下文。
    /// 支持可选的每行字符裁剪（context_chars），用于限制每行输出的最大字符数并保留匹配附近的内容。
    /// 适合在代码库中查找函数定义、变量引用、错误信息等。
    ///
    /// pattern: 搜索的正则表达式（例如 "fn build_agent" 或 "TODO"）
    /// path: 搜索范围，目录或文件路径（默认为当前目录 "."）
    /// literal: 是否精确匹配，即不使用正则表达式，默认 false（使用正则）
    /// case_sensitive: 是否大小写敏感，默认 false（忽略大小写）
    /// file_glob: 只搜索匹配此 glob 的文件（例如 "*.rs" 或 "*.{ts,tsx}"），可选
    /// context_lines: 每个匹配前后各显示几行上下文，默认 2，最多 10
    /// context_chars: 每行最多字符数，用于裁剪每行输出长度（保留匹配附近），默认 400，最多 2000
    /// max_matches: 最多返回几个匹配，默认 50，最多 200
    /// include_ignored: 是否包含 .gitignore 及隐藏文件，默认 false
    /// timeout: 超时时间，默认 20 秒
    async fn ripgrep(
        &self,
        pattern: String,
        path: Option<String>,
        literal: Option<bool>,
        case_sensitive: Option<bool>,
        file_glob: Option<String>,
        context_lines: Option<u32>,
        context_chars: Option<u32>,
        max_matches: Option<u32>,
        include_ignored: Option<bool>,
        timeout: Option<u64>,
    ) -> Value {
        let search_path = path.unwrap_or_else(|| ".".to_string());
        let context = context_lines.unwrap_or(2).min(10);
        let max = max_matches.unwrap_or(50).min(200);
        // Per-line character limit for returned lines (used to trim long lines while keeping matches).
        // Defaults to 400 and is capped at 2000.
        let context_chars_opt: Option<usize> =
            Some(context_chars.unwrap_or(400).min(2000) as usize);
        let timeout = Duration::from_secs(timeout.unwrap_or(20));

        let mut cmd = Command::new("rg");
        if literal.unwrap_or(false) {
            cmd.arg("--fixed-strings");
        }
        cmd.arg("--json");
        cmd.args(["--context", &context.to_string()]);

        if include_ignored.unwrap_or(false) {
            cmd.arg("--no-ignore");
            cmd.arg("--hidden");
        }

        if !case_sensitive.unwrap_or(false) {
            cmd.arg("--ignore-case");
        }

        if let Some(glob) = file_glob {
            cmd.args(["--glob", &glob]);
        }

        cmd.arg(&pattern);
        cmd.arg(&search_path);

        let result = match tokio::time::timeout(timeout, cmd.output()).await {
            Ok(output_res) => output_res,
            Err(_) => return json!({ "error": "command timed out" }),
        };

        let output = match result {
            Ok(o) => o,
            Err(e) => return json!({ "error": e.to_string() }),
        };

        // exit code 1 = no matches (not an error), 2 = real error
        if output.status.code() == Some(2) {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            return json!({ "error": stderr.trim().to_string() });
        }

        let stdout = String::from_utf8_lossy(&output.stdout);

        // Parse rg --json output. Each line is a JSON object with a "type" field:
        // "begin" (file start), "match" (a match), "context" (context line), "end", "summary"
        let mut matches: Vec<Value> = Vec::new();
        let mut current_file: Option<String> = None;
        let mut current_lines: Vec<Value> = Vec::new();
        let mut match_count = 0u32;

        for raw_line in stdout.lines() {
            if match_count >= max {
                break;
            }
            let obj: Value = match serde_json::from_str(raw_line) {
                Ok(v) => v,
                Err(_) => continue,
            };

            match obj["type"].as_str() {
                Some("begin") => {
                    current_file = obj["data"]["path"]["text"].as_str().map(|s| s.to_string());
                    current_lines = Vec::new();
                }
                Some("match") => {
                    let line_number = obj["data"]["line_number"].as_u64().unwrap_or(0);
                    let text_orig = obj["data"]["lines"]["text"]
                        .as_str()
                        .unwrap_or("")
                        .trim_end_matches('\n')
                        .to_string();

                    // Collect submatches for highlighting offsets
                    let submatches_arr: Vec<Value> = obj["data"]["submatches"]
                        .as_array().cloned()
                        .unwrap_or_else(std::vec::Vec::new);

                    let mut submatches: Vec<Value> = Vec::new();
                    // Determine match span (byte offsets) to center truncation around the match
                    let mut match_start = usize::MAX;
                    let mut match_end = 0usize;

                    for sm in submatches_arr.iter() {
                        let start = sm["start"].as_u64().unwrap_or(0) as usize;
                        let end = sm["end"].as_u64().unwrap_or(0) as usize;
                        let mtext = sm["match"]["text"].as_str().unwrap_or("").to_string();
                        if start < match_start {
                            match_start = start;
                        }
                        if end > match_end {
                            match_end = end;
                        }
                        submatches.push(json!({
                            "start": start,
                            "end": end,
                            "match": mtext,
                        }));
                    }

                    let mut text = text_orig.clone();
                    let mut adjusted_submatches = submatches.clone();
                    let mut truncated = false;

                    if let Some(maxc) = context_chars_opt {
                        let line_len = text_orig.len();
                        if line_len > maxc {
                            // determine window centered around match region
                            let mstart = if match_start == usize::MAX {
                                0
                            } else {
                                match_start
                            };
                            let mend = if match_end == 0 { line_len } else { match_end };
                            let center = (mstart + mend) / 2;
                            let half = maxc / 2;
                            let mut window_start = center.saturating_sub(half);
                            if window_start + maxc > line_len {
                                window_start = line_len.saturating_sub(maxc);
                            }
                            let window_end = (window_start + maxc).min(line_len);
                            // slice by byte indices; rg offsets are bytes, so this is consistent
                            text = text_orig[window_start..window_end].to_string();
                            // Adjust submatch offsets to the new sliced text
                            adjusted_submatches = adjusted_submatches
                                .into_iter()
                                .map(|sm| {
                                    let s = sm["start"].as_u64().unwrap_or(0) as i64
                                        - window_start as i64;
                                    let e = sm["end"].as_u64().unwrap_or(0) as i64
                                        - window_start as i64;
                                    let s = std::cmp::max(0, s) as usize;
                                    let e = std::cmp::min(text.len(), std::cmp::max(0, e) as usize);
                                    json!({
                                        "start": s,
                                        "end": e,
                                        "match": sm["match"].clone(),
                                    })
                                })
                                .collect();
                            truncated = window_start > 0 || window_end < line_len;
                        }
                    }

                    current_lines.push(json!({
                        "line": line_number,
                        "text": text,
                        "is_match": true,
                        "submatches": adjusted_submatches,
                        "truncated": truncated,
                    }));
                    match_count += 1;
                }
                Some("context") => {
                    let line_number = obj["data"]["line_number"].as_u64().unwrap_or(0);
                    let text_orig = obj["data"]["lines"]["text"]
                        .as_str()
                        .unwrap_or("")
                        .trim_end_matches('\n')
                        .to_string();
                    let mut text = text_orig.clone();
                    let mut truncated = false;
                    if let Some(maxc) = context_chars_opt
                        && text.len() > maxc {
                            // keep a prefix of the context line with an ellipsis marker
                            if maxc > 1 {
                                text = format!("{}…", &text_orig[..maxc.saturating_sub(1)]);
                            } else {
                                text = "…".to_string();
                            }
                            truncated = true;
                        }
                    current_lines.push(json!({
                        "line": line_number,
                        "text": text,
                        "is_match": false,
                        "truncated": truncated,
                    }));
                }
                Some("end") => {
                    if !current_lines.is_empty() {
                        matches.push(json!({
                            "file": current_file.clone().unwrap_or_default(),
                            "lines": current_lines.clone(),
                        }));
                    }
                    current_lines = Vec::new();
                }
                _ => {}
            }
        }

        // Flush any remaining lines (in case there's no trailing "end")
        if !current_lines.is_empty() {
            matches.push(json!({
                "file": current_file.unwrap_or_default(),
                "lines": current_lines,
            }));
        }

        let truncated = match_count >= max;
        json!({
            "matches": matches,
            "match_count": match_count,
            "truncated": truncated,
        })
    }

    /// 用 fd 在目录中按文件名 glob 或正则查找文件/目录。
    /// 比 find 更快，默认忽略 .gitignore 和隐藏文件。
    /// 适合在不知道完整路径时快速定位文件。
    ///
    /// pattern: 文件名匹配模式（glob 或正则，例如 "*.rs" 或 "mod.rs"）
    /// path: 搜索根目录（默认 "."）
    /// kind: 只返回 "file"、"dir" 或 "both"（默认 "both"）
    /// max_results: 最多返回几条，默认 50，最多 500
    /// include_ignored: 是否包含 .gitignore 及隐藏文件，默认 false
    async fn find_path(
        &self,
        pattern: String,
        path: Option<String>,
        kind: Option<String>,
        max_results: Option<u32>,
        include_ignored: Option<bool>,
    ) -> Value {
        let search_path = path.unwrap_or_else(|| ".".to_string());
        let max = max_results.unwrap_or(50).min(500);

        // On Debian/Ubuntu, fd is installed as `fdfind`; try fd first, fall back.
        let fd_bin = if which_exists("fd").await {
            "fd"
        } else {
            "fdfind"
        };

        let mut cmd = Command::new(fd_bin);
        // Use glob matching (default in fd is regex; --glob makes it shell-glob style)
        cmd.arg("--glob");
        cmd.args(["--max-results", &max.to_string()]);
        // Always use absolute path for clarity
        cmd.arg("--absolute-path");
        if include_ignored.unwrap_or(false) {
            // Ignore .gitignore rules and include hidden files
            cmd.arg("--no-ignore");
            cmd.arg("--hidden");
        }

        match kind.as_deref().unwrap_or("both") {
            "file" => {
                cmd.args(["--type", "f"]);
            }
            "dir" => {
                cmd.args(["--type", "d"]);
            }
            _ => {} // both: no --type filter
        }

        cmd.arg(&pattern);
        cmd.arg(&search_path);

        let output = match cmd.output().await {
            Ok(o) => o,
            Err(e) => return json!({ "error": format!("fd 执行失败: {e}") }),
        };

        if !output.status.success() && output.status.code() != Some(1) {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            return json!({ "error": stderr.trim().to_string() });
        }

        let stdout_raw = String::from_utf8_lossy(&output.stdout).to_string();
        let stdout = truncate_output(&stdout_raw, MAX_OUTPUT_CHARS);

        let paths: Vec<Value> = stdout
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| json!(l))
            .collect();

        let count = paths.len();
        let truncated = count >= max as usize;

        json!({
            "paths": paths,
            "count": count,
            "truncated": truncated,
        })
    }
}

/// Check if a binary exists in PATH without spawning a shell.
async fn which_exists(bin: &str) -> bool {
    Command::new("which")
        .arg(bin)
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false)
}
