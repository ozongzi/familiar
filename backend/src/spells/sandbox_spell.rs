use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use agentix::request::{Content, ImageContent, ImageData};
use agentix::schemars::JsonSchema;
use agentix::tool;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::sandbox::SandboxManager;

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
struct ReadItem {
    /// absolute path to the file or directory
    path: String,
    start_line: Option<usize>,
    end_line: Option<usize>,
    search_regex: Option<String>,
    context_lines: Option<usize>,
    outline_only: Option<bool>,
    extract_symbol: Option<String>,
    max_depth: Option<usize>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
struct WriteItem {
    /// absolute path to the file
    path: String,
    /// content to write or replacement text
    new_string: String,
    /// exact text to find and replace (omit to overwrite the whole file)
    old_string: Option<String>,
    /// expected number of replacements (default 1, 0 = replace all)
    count: Option<usize>,
    /// if true, append to end of file or insert after old_string
    append: Option<bool>,
    /// shebang line to prepend (e.g. "#!/usr/bin/env python3")
    shebang: Option<String>,
}

// ── constants ─────────────────────────────────────────────────────────────────

#[allow(dead_code)]
const DEFAULT_TIMEOUT_MS: u64 = 60_000;
const OUTPUT_LIMIT: usize = 8_000;
const MAX_LINES: usize = 1_000;
const MAX_DIR_DEPTH: usize = 5;
const MAX_DIR_ITEMS: usize = 50;

// ── helpers ───────────────────────────────────────────────────────────────────

fn truncate_output(s: String) -> Value {
    let total = s.len();
    if total <= OUTPUT_LIMIT {
        return json!({ "output": s, "truncated": false });
    }
    let mut cut = OUTPUT_LIMIT;
    while !s.is_char_boundary(cut) {
        cut -= 1;
    }
    json!({ "output": &s[..cut], "truncated": true, "total_bytes": total })
}

fn not_found_error(content: String) -> Value {
    let total = content.len();
    let mut cut = OUTPUT_LIMIT.min(total);
    while !content.is_char_boundary(cut) {
        cut -= 1;
    }
    json!({
        "error": "old_string not found",
        "file_content": &content[..cut],
        "truncated": total > cut,
        "total_bytes": total,
    })
}

fn build_tree(dir: &Path, prefix: &str, depth: usize, max_depth: usize) -> String {
    if depth > max_depth {
        return format!("{prefix}└── ... (max depth reached)\n");
    }
    let mut result = String::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return format!("{prefix}└── [access denied]\n"),
    };
    let mut entries: Vec<_> = entries.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|a| a.file_name());
    let total = entries.len();
    let show_count = total.min(MAX_DIR_ITEMS);
    for (i, entry) in entries.iter().take(show_count).enumerate() {
        let is_last = i == show_count - 1 && total <= MAX_DIR_ITEMS;
        let name = entry.file_name().to_string_lossy().to_string();
        let path = entry.path();
        let connector = if is_last { "└── " } else { "├── " };
        if path.is_dir() {
            result.push_str(&format!("{prefix}{connector}{name}/\n"));
            let new_prefix = if is_last { format!("{prefix}    ") } else { format!("{prefix}│   ") };
            result.push_str(&build_tree(&path, &new_prefix, depth + 1, max_depth));
        } else {
            result.push_str(&format!("{prefix}{connector}{name}\n"));
        }
    }
    if total > MAX_DIR_ITEMS {
        result.push_str(&format!("{prefix}└── ... ({} more items)\n", total - MAX_DIR_ITEMS));
    }
    result
}

fn run_ctags(path: &Path) -> Option<String> {
    let output = std::process::Command::new("ctags")
        .args(["-f", "-", "--fields=n", "--sort=no", path.to_str()?])
        .output()
        .ok()?;
    if !output.status.success() { return None; }
    String::from_utf8(output.stdout).ok()
}

fn parse_ctags_output(s: &str) -> Vec<(String, String, usize)> {
    s.lines().filter_map(|line| {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 3 { return None; }
        let name = parts[0].to_string();
        let kind = parts.get(3).and_then(|s| s.strip_prefix("kind:")).unwrap_or("?").to_string();
        let ln = parts.last().and_then(|s| s.strip_prefix("line:")).and_then(|s| s.parse().ok()).unwrap_or(0);
        Some((name, kind, ln))
    }).collect()
}

fn add_line_numbers(content: &str, start_line: usize) -> String {
    content.lines().enumerate()
        .map(|(i, l)| format!("{:4} | {}", start_line + i, l))
        .collect::<Vec<_>>().join("\n")
}

fn image_mime(path: &Path) -> Option<&'static str> {
    match path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref() {
        Some("png")  => Some("image/png"),
        Some("jpg") | Some("jpeg") => Some("image/jpeg"),
        Some("gif")  => Some("image/gif"),
        Some("webp") => Some("image/webp"),
        Some("bmp")  => Some("image/bmp"),
        Some("svg")  => Some("image/svg+xml"),
        _ => None,
    }
}

async fn read_as_contents(path: &str) -> Vec<Content> {
    use base64::Engine as _;
    let p = Path::new(path);
    if let Some(mime) = image_mime(p) {
        match tokio::fs::read(p).await {
            Ok(bytes) => {
                let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                vec![
                    Content::Image(ImageContent {
                        data: ImageData::Base64(b64),
                        mime_type: mime.to_string(),
                    }),
                    Content::text(json!({ "type": "image", "path": path, "mime_type": mime }).to_string()),
                ]
            }
            Err(e) => vec![Content::text(json!({ "error": format!("read failed: {e}"), "path": path }).to_string())],
        }
    } else {
        let result = do_read(path, None, None, None, None, None, None, None).await;
        vec![Content::text(result.to_string())]
    }
}

#[allow(clippy::too_many_arguments)]
async fn do_read(
    path: &str,
    start_line: Option<usize>,
    end_line: Option<usize>,
    outline_only: Option<bool>,
    extract_symbol: Option<String>,
    max_depth: Option<usize>,
    search_regex: Option<String>,
    context_lines: Option<usize>,
) -> Value {
    let p = Path::new(path);
    if p.is_dir() {
        let tree = build_tree(p, "", 0, max_depth.unwrap_or(3).min(MAX_DIR_DEPTH));
        return json!({ "type": "directory", "path": path, "tree": tree });
    }
    if !p.exists() {
        return json!({ "error": format!("file not found: {path}") });
    }
    let content = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => return json!({ "error": format!("read failed: {e}") }),
    };
    if outline_only.unwrap_or(false) {
        if let Some(ctags) = run_ctags(p) {
            let tags = parse_ctags_output(&ctags);
            let outline = tags.iter().map(|(n, k, l)| format!("{l:4} | [{k}] {n}")).collect::<Vec<_>>().join("\n");
            return json!({ "type": "outline", "path": path, "outline": outline });
        }
        return json!({ "type": "outline", "path": path, "outline": "(ctags not available or no symbols found)" });
    }
    if let Some(symbol) = extract_symbol {
        if let Some(ctags) = run_ctags(p) {
            let tags = parse_ctags_output(&ctags);
            if let Some((_, _, tl)) = tags.iter().find(|(n, _, _)| n == &symbol) {
                let tl = *tl;
                let lines: Vec<&str> = content.lines().collect();
                let end = tags.iter().filter(|(_, _, l)| *l > tl).map(|(_, _, l)| *l).min().unwrap_or(lines.len());
                if let Some(body) = lines.get(tl - 1..end.saturating_sub(1)) {
                    let joined = body.join("\n");
                    return json!({ "type": "symbol", "path": path, "symbol": symbol, "content": add_line_numbers(&joined, 1) });
                }
            }
        }
        return json!({ "error": format!("symbol '{symbol}' not found") });
    }
    if let Some(regex) = search_regex {
        let ctx = context_lines.unwrap_or(2);
        let re = match regex::Regex::new(&regex) {
            Ok(r) => r,
            Err(e) => return json!({ "error": format!("invalid regex: {e}") }),
        };
        let lines: Vec<&str> = content.lines().collect();
        let matched: Vec<usize> = lines.iter().enumerate().filter(|(_, l)| re.is_match(l)).map(|(i, _)| i).collect();
        let mut shown = std::collections::BTreeSet::new();
        for &m in &matched { for i in m.saturating_sub(ctx)..(m + ctx + 1).min(lines.len()) { shown.insert(i); } }
        let mut out = String::new();
        for i in shown {
            let marker = if matched.contains(&i) { ">>>" } else { "   " };
            out.push_str(&format!("{marker} {:4} | {}\n", i + 1, lines[i]));
        }
        return json!({ "path": path, "pattern": regex, "total_matches": matched.len(), "matches": out.trim_end() });
    }
    let lines: Vec<&str> = content.lines().collect();
    let total_lines = lines.len();
    let start = start_line.map(|s| s.saturating_sub(1)).unwrap_or(0);
    let end = end_line.map(|e| e.min(total_lines)).unwrap_or(total_lines);
    let selected: Vec<&str> = lines.get(start..end).unwrap_or_default().to_vec();
    let truncated = selected.len() > MAX_LINES;
    let show: Vec<&str> = if truncated { selected.iter().take(MAX_LINES).copied().collect() } else { selected };
    json!({
        "type": "file", "path": path,
        "content": add_line_numbers(&show.join("\n"), start + 1),
        "start_line": start + 1,
        "end_line": (start + show.len()).min(total_lines),
        "total_lines": total_lines,
        "truncated": truncated,
        "max_lines": MAX_LINES,
    })
}

// ── write ─────────────────────────────────────────────────────────────────────

async fn do_write(
    path: &str,
    new_string: String,
    old_string: Option<String>,
    count: Option<usize>,
    append: Option<bool>,
    shebang: Option<String>,
) -> Value {
    let new_string = if let Some(ref s) = shebang {
        let line = if s.starts_with("#!") { s.clone() } else { format!("#!/usr/bin/env {s}") };
        format!("{line}\n{new_string}")
    } else {
        new_string
    };

    if append.unwrap_or(false) {
        if let Some(ref anchor) = old_string {
            let original = match std::fs::read_to_string(path) {
                Ok(s) => s,
                Err(e) => return json!({ "error": format!("read failed: {e}") }),
            };
            if !original.contains(anchor.as_str()) {
                return not_found_error(original);
            }
            let updated = original.replacen(anchor.as_str(), &format!("{anchor}{new_string}"), 1);
            if let Err(e) = std::fs::write(path, &updated) {
                return json!({ "error": format!("write failed: {e}") });
            }
            return json!({ "inserted_after": path, "bytes": new_string.len() });
        }
        use std::fs::OpenOptions;
        use std::io::Write;
        match OpenOptions::new().create(true).append(true).open(path) {
            Err(e) => return json!({ "error": format!("open failed: {e}") }),
            Ok(mut f) => {
                if let Err(e) = f.write_all(new_string.as_bytes()) {
                    return json!({ "error": format!("write failed: {e}") });
                }
            }
        }
        return json!({ "appended": path, "bytes": new_string.len() });
    }

    if let Some(old) = old_string {
        let original = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => return json!({ "error": format!("read failed: {e}") }),
        };
        let found = original.matches(old.as_str()).count();
        if found == 0 {
            return not_found_error(original);
        }
        let expected = count.unwrap_or(1);
        if expected != 0 && found != expected {
            return json!({ "error": format!("expected {expected} occurrence(s) but found {found}") });
        }
        let updated = if expected == 0 {
            original.replace(old.as_str(), &new_string)
        } else {
            let mut s = original.clone();
            for _ in 0..expected {
                s = s.replacen(old.as_str(), &new_string, 1);
            }
            s
        };
        if let Err(e) = std::fs::write(path, &updated) {
            return json!({ "error": format!("write failed: {e}") });
        }
        return json!({ "replaced": path, "occurrences": found });
    }

    if let Some(parent) = Path::new(path).parent()
        && !parent.as_os_str().is_empty()
        && let Err(e) = std::fs::create_dir_all(parent) {
            return json!({ "error": format!("mkdir failed: {e}") });
        }
    if let Err(e) = std::fs::write(path, &new_string) {
        return json!({ "error": format!("write failed: {e}") });
    }
    json!({ "written": path, "bytes": new_string.len() })
}

// ── autocheck ─────────────────────────────────────────────────────────────────

#[derive(Debug)]
enum Language { Rust, Go, Python, JavaScript }

fn detect_language(path: &Path) -> Option<Language> {
    let ext = path.extension().and_then(|e| e.to_str());
    match ext {
        Some("rs") => return Some(Language::Rust),
        Some("go") => return Some(Language::Go),
        Some("py") => return Some(Language::Python),
        Some("js") | Some("ts") | Some("jsx") | Some("tsx") => return Some(Language::JavaScript),
        _ => {}
    }
    let name = path.file_name().and_then(|n| n.to_str())?;
    match name {
        "Cargo.toml" => Some(Language::Rust),
        "go.mod" => Some(Language::Go),
        "package.json" => Some(Language::JavaScript),
        _ => None,
    }
}

fn root_markers(lang: &Language) -> &'static [&'static str] {
    match lang {
        Language::Rust => &["Cargo.toml"],
        Language::Go => &["go.mod"],
        Language::Python => &["pyproject.toml", "requirements.txt", "setup.py", ".git"],
        Language::JavaScript => &["package.json", "tsconfig.json", ".git"],
    }
}

fn find_root(start: &Path, markers: &[&str]) -> Option<PathBuf> {
    let mut cur = if start.is_file() { start.parent()?.to_path_buf() } else { start.to_path_buf() };
    loop {
        for marker in markers {
            if cur.join(marker).exists() { return Some(cur); }
        }
        if !cur.pop() { return None; }
    }
}

// fn path_env_with_cargo() -> String {
//     let current = std::env::var("PATH").unwrap_or_default();
//     let cargo_bin = std::env::var("HOME").map(|h| format!("{h}/.cargo/bin")).unwrap_or_default();
//     if cargo_bin.is_empty() || current.contains(&cargo_bin) { current }
//     else { format!("{cargo_bin}:{current}") }
// }

fn parse_rust_diagnostics(stderr: &str, crate_root: &Path) -> Vec<Value> {
    let mut diags: Vec<Value> = Vec::new();
    let lines: Vec<&str> = stderr.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let level = if line.starts_with("error") { "error" }
            else if line.starts_with("warning") { "warning" }
            else { i += 1; continue; };
        if line.contains("aborting due to") || line.contains("could not compile") { i += 1; continue; }
        let message = line.split_once(": ").map(|x| x.1).unwrap_or(line).trim().to_string();
        let mut location: Option<(String, usize, usize)> = None;
        let mut j = i + 1;
        while j < lines.len() && j < i + 6 {
            let loc = lines[j].trim();
            if let Some(rest) = loc.strip_prefix("--> ") {
                let p: Vec<&str> = rest.splitn(3, ':').collect();
                if p.len() >= 2
                    && let Ok(row) = p[1].parse::<usize>() {
                        let col = p.get(2).and_then(|s| s.parse().ok()).unwrap_or(1);
                        location = Some((p[0].to_string(), row, col));
                    }
                break;
            }
            if lines[j].starts_with("error") || lines[j].starts_with("warning") { break; }
            j += 1;
        }
        let mut k = i + 1;
        let mut raw_lines = vec![line];
        while k < lines.len() {
            let next = lines[k];
            let is_new = !next.starts_with(' ') && !next.starts_with('\t')
                && (next.starts_with("error") || next.starts_with("warning"))
                && !next.trim().is_empty();
            if is_new { break; }
            raw_lines.push(next);
            k += 1;
        }
        let source_context = location.as_ref().and_then(|(rel, row, _)| {
            let abs = if Path::new(rel).is_absolute() { PathBuf::from(rel) } else { crate_root.join(rel) };
            let src = std::fs::read_to_string(&abs).ok()?;
            let src_lines: Vec<&str> = src.lines().collect();
            let center = row.saturating_sub(1);
            let start = center.saturating_sub(5);
            let end = (center + 6).min(src_lines.len());
            let snippet: Vec<String> = src_lines[start..end].iter().enumerate().map(|(idx, l)| {
                let lineno = start + idx + 1;
                let marker = if lineno == *row { ">>>" } else { "   " };
                format!("{marker} {lineno:4} | {l}")
            }).collect();
            Some(json!({ "file": rel, "line": row, "snippet": snippet.join("\n") }))
        });
        let mut diag = json!({ "level": level, "message": message, "raw": raw_lines.join("\n") });
        if let Some((f, r, c)) = &location {
            diag["file"] = json!(f); diag["line"] = json!(r); diag["col"] = json!(c);
        }
        if let Some(ctx) = source_context { diag["source_context"] = ctx; }
        diags.push(diag);
        i = k;
    }
    diags
}

fn parse_generic_diagnostics(output: &str, root: &Path) -> Vec<Value> {
    let re = regex::Regex::new(r"(?m)^(.+?):(\d+)(?::(\d+))?:?\s*(.*)$").unwrap();
    let mut diags = Vec::new();
    for cap in re.captures_iter(output) {
        let file = cap.get(1).map(|m| m.as_str().to_string()).unwrap_or_default();
        let line = cap.get(2).and_then(|m| m.as_str().parse::<usize>().ok()).unwrap_or(1);
        let col = cap.get(3).and_then(|m| m.as_str().parse::<usize>().ok()).unwrap_or(1);
        let message = cap.get(4).map(|m| m.as_str().to_string()).unwrap_or_default();
        if file.is_empty() || message.is_empty() { continue; }
        let abs_path = if Path::new(&file).is_absolute() { PathBuf::from(&file) } else { root.join(&file) };
        if !abs_path.exists() { continue; }
        let level = if message.to_lowercase().contains("error") { "error" } else { "warning" };
        let mut diag = json!({ "file": file, "line": line, "col": col, "message": message, "level": level });
        if let Ok(src) = std::fs::read_to_string(&abs_path) {
            let src_lines: Vec<&str> = src.lines().collect();
            let center = line.saturating_sub(1);
            if center < src_lines.len() {
                let start = center.saturating_sub(2);
                let end = (center + 3).min(src_lines.len());
                let snippet: Vec<String> = src_lines[start..end].iter().enumerate().map(|(idx, l)| {
                    let lineno = start + idx + 1;
                    let marker = if lineno == line { ">>>" } else { "   " };
                    format!("{marker} {lineno:4} | {l}")
                }).collect();
                diag["source_context"] = json!({ "file": file, "line": line, "snippet": snippet.join("\n") });
            }
        }
        diags.push(diag);
    }
    diags
}

async fn run_autocheck_in_container(
    sandbox: &SandboxManager,
    user_id: Uuid,
    conversation_id: Uuid,
    path: &str,
) -> Option<Value> {
    let p = Path::new(path);
    let lang = detect_language(p)?;
    let markers = root_markers(&lang);
    let root = find_root(p, markers)?;
    let root_str = root.to_str()?;

    let check_cmd = match lang {
        Language::Rust => format!("cd {root_str} && cargo clippy --message-format=human 2>&1"),
        Language::Go => format!("cd {root_str} && go vet ./... 2>&1"),
        Language::Python => format!("cd {root_str} && ruff check . 2>&1 || python3 -m py_compile {path} 2>&1"),
        Language::JavaScript => format!("cd {root_str} && npx --yes eslint . 2>&1"),
    };

    let (prog, args) = sandbox.wrap_mcp_command(user_id, conversation_id, "sh", &["-c", &check_cmd]);
    let output = tokio::process::Command::new(&prog)
        .args(&args)
        .output()
        .await
        .ok()?;

    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let success = output.status.success();

    let diags: Vec<Value> = match lang {
        Language::Rust => parse_rust_diagnostics(&combined, &root),
        _ => parse_generic_diagnostics(&combined, &root),
    };
    let errors: Vec<Value> = diags.iter().filter(|d| d["level"] == "error").cloned().collect();
    let warnings: Vec<Value> = diags.iter().filter(|d| d["level"] == "warning").cloned().collect();

    let summary = if success {
        format!("✅ check passed ({} warning(s))", warnings.len())
    } else {
        format!("❌ check failed: {} error(s), {} warning(s)", errors.len(), warnings.len())
    };

    Some(json!({ "success": success, "fix_ok": false, "summary": summary, "errors": errors, "warnings": warnings }))
}

// ── bash streaming via docker exec ────────────────────────────────────────────

enum BashOutput { Line(String), Done(Value) }

fn run_bash_in_container_streaming(
    sandbox: Arc<SandboxManager>,
    user_id: Uuid,
    conversation_id: Uuid,
    command: String,
    timeout_ms: u64,
) -> impl futures::Stream<Item = BashOutput> {
    async_stream::stream! {
        use tokio::io::AsyncBufReadExt;

        let (prog, args) = sandbox.wrap_mcp_command(user_id, conversation_id, "sh", &["-c", &command]);
        let mut child = match tokio::process::Command::new(&prog)
            .args(&args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => { yield BashOutput::Done(json!({ "error": format!("spawn failed: {e}") })); return; }
        };

        let stdout = child.stdout.take().expect("piped");
        let stderr = child.stderr.take().expect("piped");
        let mut stdout_lines = tokio::io::BufReader::new(stdout).lines();
        let mut stderr_lines = tokio::io::BufReader::new(stderr).lines();
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_millis(timeout_ms);
        let mut output_buf = String::new();
        let mut timed_out = false;

        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() { timed_out = true; let _ = child.kill().await; break; }
            tokio::select! {
                line = stdout_lines.next_line() => {
                    match line {
                        Ok(Some(l)) => { output_buf.push_str(&l); output_buf.push('\n'); yield BashOutput::Line(l); }
                        _ => break,
                    }
                }
                line = stderr_lines.next_line() => {
                    if let Ok(Some(l)) = line {
                        output_buf.push_str(&l); output_buf.push('\n');
                        yield BashOutput::Line(format!("[stderr] {l}"));
                    }
                }
                _ = tokio::time::sleep_until(deadline) => { timed_out = true; let _ = child.kill().await; break; }
            }
        }

        if timed_out {
            yield BashOutput::Done(json!({ "error": format!("timed out after {timeout_ms}ms"), "timed_out": true }));
            return;
        }
        let exit_code = child.wait().await.ok().and_then(|s| s.code());
        let mut r = truncate_output(output_buf);
        r["exit_code"] = json!(exit_code);
        r["timed_out"] = json!(false);
        yield BashOutput::Done(r);
    }
}

// ── SandboxSpell Tool ─────────────────────────────────────────────────────────

/// Built-in file system tools (read/write/diff/grep) plus sandboxed bash execution.
///
/// File operations run directly on the host filesystem inside the artifacts
/// directory. `bash` and `autocheck` commands are forwarded into the per-
/// conversation Docker sandbox container via `docker exec` (Docker-out-of-Docker).
pub struct SandboxSpell {
    pub sandbox: Arc<SandboxManager>,
    pub user_id: Uuid,
    pub conversation_id: Uuid,
}

impl SandboxSpell {
    /// Translate a `/workspace/...` path to an absolute host path, with security checks:
    /// - Non-/workspace absolute paths are rejected (container-only paths)
    /// - Path traversal (e.g. /workspace/../etc/passwd) is rejected
    fn resolve_path(&self, path: &str) -> Result<String, Value> {
        // Reject absolute paths outside /workspace
        if Path::new(path).is_absolute() && !path.starts_with("/workspace") {
            return Err(json!({ "error": format!("path '{path}' is outside /workspace. Use the bash tool instead (e.g. cat {path})") }));
        }

        let conv_dir = self.sandbox.get_conversation_dir(self.user_id, self.conversation_id);
        let resolved = if let Some(rest) = path.strip_prefix("/workspace") {
            let rest = rest.trim_start_matches('/');
            if rest.is_empty() {
                conv_dir.clone()
            } else {
                conv_dir.join(rest)
            }
        } else {
            conv_dir.join(path)
        };

        // Normalize away any .. without touching the filesystem
        let mut normalized = PathBuf::new();
        for component in resolved.components() {
            match component {
                std::path::Component::ParentDir => { normalized.pop(); }
                std::path::Component::CurDir => {}
                c => normalized.push(c),
            }
        }

        // After normalization, must still be inside conv_dir
        if !normalized.starts_with(&conv_dir) {
            return Err(json!({ "error": format!("path '{path}' resolves outside /workspace (path traversal denied)") }));
        }

        Ok(normalized.to_string_lossy().into_owned())
    }

    /// Returns Err if the path is not under /workspace (i.e. not accessible on the host).
    fn require_workspace_path(&self, path: &str) -> Result<String, Value> {
        self.resolve_path(path)
    }

    /// Reverse of resolve_path: translate an absolute host path back to the
    /// `/workspace/...` form that the agent and sandbox container understand.
    /// Paths outside the conversation directory are returned unchanged.
    fn unresolve_path(&self, host_path: &str) -> String {
        let conv_dir = self.sandbox.get_conversation_dir(self.user_id, self.conversation_id);
        let conv_str = conv_dir.to_string_lossy();
        if let Some(rest) = host_path.strip_prefix(conv_str.as_ref()) {
            let rest = rest.trim_start_matches('/');
            if rest.is_empty() {
                "/workspace".to_owned()
            } else {
                format!("/workspace/{rest}")
            }
        } else {
            host_path.to_owned()
        }
    }

    /// Rewrite any path-valued fields in a tool result JSON from host paths
    /// back to `/workspace/...` paths.
    fn fixup_result_paths(&self, mut result: Value) -> Value {
        for key in &["written", "replaced", "inserted_after", "appended", "path"] {
            if let Some(Value::String(s)) = result.get(*key) {
                let fixed = self.unresolve_path(s);
                result[key] = Value::String(fixed);
            }
        }
        result
    }
}

#[tool]
impl agentix::Tool for SandboxSpell {
    /// Read, search, explore, or summarize files and directories.
    /// Image files (png, jpg, gif, webp, bmp, svg) are returned as viewable images.
    ///
    /// path: absolute path to the file or directory
    /// start_line: optional starting line number (1-indexed)
    /// end_line: optional ending line number
    /// search_regex: regex to search for in the file
    /// context_lines: lines of context around each match (default: 2)
    /// outline_only: if true, return only structural signatures via ctags
    /// extract_symbol: extract the full body of a specific function or class
    /// max_depth: maximum depth for directory tree view (default: 3, max: 5)
    async fn read(
        &self,
        path: String,
        start_line: Option<usize>,
        end_line: Option<usize>,
        search_regex: Option<String>,
        context_lines: Option<usize>,
        outline_only: Option<bool>,
        extract_symbol: Option<String>,
        max_depth: Option<usize>,
    ) -> Vec<Content> {
        let path = match self.require_workspace_path(&path) {
            Ok(p) => p,
            Err(e) => return vec![Content::text(e.to_string())],
        };
        if image_mime(Path::new(&path)).is_some() {
            return read_as_contents(&path).await;
        }
        let result = do_read(&path, start_line, end_line, outline_only, extract_symbol, max_depth, search_regex, context_lines).await;
        vec![Content::text(self.fixup_result_paths(result).to_string())]
    }

    /// Read multiple files or directories in one call to reduce round-trips.
    /// Image files are returned inline as viewable images.
    /// reads: array of read operations; each item has: path (required), start_line, end_line, search_regex, context_lines, outline_only, extract_symbol, max_depth
    async fn multiread(&self, reads: Vec<ReadItem>) -> Vec<Content> {
        let mut contents: Vec<Content> = Vec::new();
        for item in reads {
            let path = match self.require_workspace_path(&item.path) {
                Ok(resolved) => resolved,
                Err(e) => { contents.push(Content::text(e.to_string())); continue; }
            };
            if image_mime(Path::new(&path)).is_some() {
                contents.extend(read_as_contents(&path).await);
            } else {
                let result = self.fixup_result_paths(do_read(
                    &path,
                    item.start_line,
                    item.end_line,
                    item.outline_only,
                    item.extract_symbol,
                    item.max_depth,
                    item.search_regex,
                    item.context_lines,
                ).await);
                contents.push(Content::text(result.to_string()));
            }
        }
        contents
    }

    /// Write to a file (overwrite / replace / append), then run autocheck in the sandbox.
    ///
    /// path: absolute path to the file
    /// new_string: content to write or replacement text
    /// old_string: exact text to find and replace (or anchor for insert-after)
    /// count: expected number of replacements (default 1, 0 = replace all)
    /// append: if true, append to end of file or insert after old_string
    /// shebang: if provided, prepend this shebang line
    async fn write(
        &self,
        path: String,
        new_string: String,
        old_string: Option<String>,
        count: Option<usize>,
        append: Option<bool>,
        shebang: Option<String>,
    ) -> Value {
        let path = match self.require_workspace_path(&path) {
            Ok(p) => p,
            Err(e) => return e,
        };
        let mut result = do_write(&path, new_string, old_string, count, append, shebang).await;
        if result.get("error").is_none() {
            result = self.fixup_result_paths(result);
            if let Some(ac) = run_autocheck_in_container(&self.sandbox, self.user_id, self.conversation_id, &path).await {
                result["autocheck"] = ac;
            }
        }
        result
    }

    /// Write multiple files in one call, then run checks for all affected projects.
    /// writes: array of write operations; each item has: path (required), new_string (required), old_string, count, append, shebang
    async fn multiwrite(&self, writes: Vec<WriteItem>) -> Value {
        let mut results = Vec::new();
        let mut affected: HashSet<(PathBuf, String)> = HashSet::new();
        let mut failures: Vec<String> = Vec::new();

        for item in &writes {
            let path = match self.require_workspace_path(&item.path) {
                Ok(resolved) => resolved,
                Err(e) => { results.push(e); failures.push(item.path.clone()); continue; }
            };
            let write_result = do_write(
                &path,
                item.new_string.clone(),
                item.old_string.clone(),
                item.count,
                item.append,
                item.shebang.clone(),
            ).await;
            let failed = write_result.get("error").is_some();
            let write_result = if failed { write_result } else { self.fixup_result_paths(write_result) };
            results.push(write_result);
            if failed { failures.push(path.clone()); continue; }
            let p = Path::new(&path);
            if let Some(lang) = detect_language(p) {
                let markers = root_markers(&lang);
                if let Some(root) = find_root(p, markers) {
                    affected.insert((root, format!("{lang:?}")));
                }
            }
        }

        let mut autochecks = Vec::new();
        for (root, _) in &affected {
            if let Some(first_path) = results.iter().zip(writes.iter())
                .filter(|(r, _)| r.get("error").is_none())
                .map(|(_, w)| w.path.as_str())
                .find(|p| Path::new(p).starts_with(root))
                && let Some(ac) = run_autocheck_in_container(&self.sandbox, self.user_id, self.conversation_id, first_path).await {
                    autochecks.push(ac);
                }
        }

        json!({ "results": results, "failed_paths": failures, "autochecks": autochecks })
    }

    /// Compare two files and return their differences.
    /// path1: path to the first file
    /// path2: path to the second file
    async fn diff(&self, path1: String, path2: String) -> Value {
        let text1 = std::fs::read_to_string(&path1).unwrap_or_default();
        let text2 = std::fs::read_to_string(&path2).unwrap_or_default();
        let diff = similar::TextDiff::from_lines(&text1, &text2);
        let result = diff.unified_diff()
            .header(&format!("a/{path1}"), &format!("b/{path2}"))
            .to_string();
        if result.is_empty() {
            json!({ "diff": "(no changes)" })
        } else if result.len() > OUTPUT_LIMIT {
            let mut cut = OUTPUT_LIMIT;
            while !result.is_char_boundary(cut) { cut -= 1; }
            json!({ "diff": format!("{}... (truncated)", &result[..cut]) })
        } else {
            json!({ "diff": result })
        }
    }

    /// Run a shell command inside the sandbox container and stream its output.
    /// command: the shell command to execute
    /// timeout_ms: optional timeout in milliseconds (default: 60000)
    #[streaming]
    fn bash(&self, command: String, timeout_ms: Option<u64>) {
        let sandbox = self.sandbox.clone();
        let user_id = self.user_id;
        let conversation_id = self.conversation_id;
        async_stream::stream! {
            use agentix::ToolOutput;
            use futures::StreamExt;
            let mut stream = std::pin::pin!(run_bash_in_container_streaming(
                sandbox, user_id, conversation_id,
                command, timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS),
            ));
            while let Some(item) = stream.next().await {
                match item {
                    BashOutput::Line(l) => yield ToolOutput::Progress(l),
                    BashOutput::Done(r) => yield ToolOutput::Result(vec![agentix::Content::text(serde_json::to_string(&r).unwrap_or_default())]),
                }
            }
        }
    }
}
