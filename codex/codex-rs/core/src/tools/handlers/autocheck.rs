//! Synchronous post-edit linting / type-checking for files touched by tools
//! that mutate the filesystem (currently `apply_patch`).
//!
//! Why synchronous? Letting the model issue a separate "now run the linter"
//! tool call doubles the token spend on the turn (full context replay). Doing
//! it inline — append the diagnostics to the same tool result — lets the
//! model see errors and fix them in the same turn.
//!
//! Ported from familiar's `backend/src/spells/sandbox_spell.rs`
//! (`run_autocheck_in_container` / `parse_rust_diagnostics` /
//! `parse_generic_diagnostics`). Trimmed to the host-process variant: codex
//! runs on the host (no Docker indirection) and uses its existing approval
//! and sandbox layers separately.

use std::collections::BTreeSet;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use serde_json::Value;
use serde_json::json;

/// Generous bound — a cold `cargo clippy` on a medium crate can take 30-60s.
/// Still bounded so a runaway cargo doesn't hang the turn.
const AUTOCHECK_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum Language {
    Rust,
    Go,
    Python,
    JavaScript,
}

fn detect_language(path: &Path) -> Option<Language> {
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        match ext {
            "rs" => return Some(Language::Rust),
            "go" => return Some(Language::Go),
            "py" => return Some(Language::Python),
            "js" | "ts" | "jsx" | "tsx" | "mjs" | "cjs" => return Some(Language::JavaScript),
            _ => {}
        }
    }
    let name = path.file_name().and_then(|n| n.to_str())?;
    match name {
        "Cargo.toml" => Some(Language::Rust),
        "go.mod" => Some(Language::Go),
        "package.json" => Some(Language::JavaScript),
        _ => None,
    }
}

fn root_markers(lang: Language) -> &'static [&'static str] {
    match lang {
        Language::Rust => &["Cargo.toml"],
        Language::Go => &["go.mod"],
        Language::Python => &["pyproject.toml", "requirements.txt", "setup.py", ".git"],
        Language::JavaScript => &["package.json", "tsconfig.json", ".git"],
    }
}

fn find_root(start: &Path, markers: &[&str]) -> Option<PathBuf> {
    let mut cur = if start.is_file() {
        start.parent()?.to_path_buf()
    } else {
        start.to_path_buf()
    };
    loop {
        for marker in markers {
            if cur.join(marker).exists() {
                return Some(cur);
            }
        }
        if !cur.pop() {
            return None;
        }
    }
}

/// Run autocheck on every distinct (project_root, language) reachable from
/// the supplied files. Returns a markdown-style summary block, or `None` if
/// no files matched a known language.
///
/// The check command is run with the project root as cwd. Each (root, lang)
/// pair is processed serially to keep stdout interleaving readable.
pub async fn run_autocheck_for_paths(paths: &[PathBuf], session_cwd: &Path) -> Option<String> {
    let mut groups: BTreeSet<(PathBuf, Language)> = BTreeSet::new();
    for p in paths {
        let abs: PathBuf = if p.is_absolute() {
            p.clone()
        } else {
            session_cwd.join(p)
        };
        let lang = match detect_language(&abs) {
            Some(l) => l,
            None => continue,
        };
        let markers = root_markers(lang);
        let Some(root) = find_root(&abs, markers) else {
            continue;
        };
        groups.insert((root, lang));
    }

    if groups.is_empty() {
        return None;
    }

    // BTreeSet doesn't impl Ord on Language directly — we used a manual
    // ordering above by deriving on enum variant order. Collect and run.
    let mut sections: Vec<String> = Vec::new();
    for (root, lang) in groups {
        let result = run_one_check(&root, lang).await;
        sections.push(format_section(&root, lang, result));
    }

    Some(sections.join("\n\n"))
}

#[derive(Debug)]
struct CheckResult {
    success: bool,
    errors: Vec<Value>,
    warnings: Vec<Value>,
    raw_excerpt: Option<String>,
}

async fn run_one_check(root: &Path, lang: Language) -> CheckResult {
    let (program, args): (&str, Vec<&str>) = match lang {
        Language::Rust => ("cargo", vec!["clippy", "--message-format=human"]),
        Language::Go => ("go", vec!["vet", "./..."]),
        Language::Python => ("ruff", vec!["check", "."]),
        Language::JavaScript => ("npx", vec!["--yes", "eslint", "."]),
    };

    let output = match tokio::time::timeout(
        AUTOCHECK_TIMEOUT,
        tokio::process::Command::new(program)
            .args(&args)
            .current_dir(root)
            .output(),
    )
    .await
    {
        Err(_elapsed) => {
            return CheckResult {
                success: false,
                errors: vec![json!({
                    "level": "error",
                    "message": format!("autocheck timed out after {}s", AUTOCHECK_TIMEOUT.as_secs()),
                })],
                warnings: Vec::new(),
                raw_excerpt: None,
            };
        }
        Ok(Err(e)) => {
            // Tool not installed (rustc/cargo/go/ruff/npx missing) — this
            // is a soft failure, not a code problem. Surface it as a single
            // warning so the model knows the check was unavailable.
            return CheckResult {
                success: true,
                errors: Vec::new(),
                warnings: vec![json!({
                    "level": "warning",
                    "message": format!("autocheck skipped: failed to run {program}: {e}"),
                })],
                raw_excerpt: None,
            };
        }
        Ok(Ok(o)) => o,
    };

    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let success = output.status.success();

    let diags = match lang {
        Language::Rust => parse_rust_diagnostics(&combined, root),
        _ => parse_generic_diagnostics(&combined, root),
    };
    let (errors, warnings): (Vec<_>, Vec<_>) = diags.into_iter().partition(|d| {
        d.get("level").and_then(|v| v.as_str()) == Some("error")
    });

    let raw_excerpt = if !success && errors.is_empty() && warnings.is_empty() {
        Some(truncate(&combined, 4_000))
    } else {
        None
    };

    CheckResult {
        success,
        errors,
        warnings,
        raw_excerpt,
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut cut = max;
    while !s.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}\n... (output truncated)", &s[..cut])
}

fn format_section(root: &Path, lang: Language, r: CheckResult) -> String {
    let lang_name = match lang {
        Language::Rust => "rust (cargo clippy)",
        Language::Go => "go (go vet)",
        Language::Python => "python (ruff)",
        Language::JavaScript => "javascript (eslint)",
    };
    let header = if r.success {
        format!(
            "✅ autocheck [{}] @ {}: passed ({} warning(s))",
            lang_name,
            root.display(),
            r.warnings.len()
        )
    } else {
        format!(
            "❌ autocheck [{}] @ {}: failed ({} error(s), {} warning(s))",
            lang_name,
            root.display(),
            r.errors.len(),
            r.warnings.len()
        )
    };

    let mut out = header;
    for d in r.errors.iter().chain(r.warnings.iter()) {
        out.push('\n');
        out.push_str(&format_diagnostic(d));
    }
    if let Some(raw) = r.raw_excerpt {
        out.push_str("\n\n--- raw output ---\n");
        out.push_str(&raw);
    }
    out
}

fn format_diagnostic(d: &Value) -> String {
    let level = d.get("level").and_then(|v| v.as_str()).unwrap_or("info");
    let message = d.get("message").and_then(|v| v.as_str()).unwrap_or("");
    let mut out = match (
        d.get("file").and_then(|v| v.as_str()),
        d.get("line").and_then(|v| v.as_u64()),
    ) {
        (Some(file), Some(line)) => format!("[{level}] {file}:{line}: {message}"),
        _ => format!("[{level}] {message}"),
    };
    if let Some(ctx) = d.get("source_context").and_then(|v| v.get("snippet")).and_then(|v| v.as_str()) {
        out.push('\n');
        out.push_str(ctx);
    }
    out
}

// ── Rust diagnostic parsing ──────────────────────────────────────────────────

fn parse_rust_diagnostics(stderr: &str, crate_root: &Path) -> Vec<Value> {
    let mut diags: Vec<Value> = Vec::new();
    let lines: Vec<&str> = stderr.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let level = if line.starts_with("error") {
            "error"
        } else if line.starts_with("warning") {
            "warning"
        } else {
            i += 1;
            continue;
        };
        if line.contains("aborting due to") || line.contains("could not compile") {
            i += 1;
            continue;
        }
        let message = line
            .split_once(": ")
            .map(|x| x.1)
            .unwrap_or(line)
            .trim()
            .to_string();
        let mut location: Option<(String, usize, usize)> = None;
        let mut j = i + 1;
        while j < lines.len() && j < i + 6 {
            let loc = lines[j].trim();
            if let Some(rest) = loc.strip_prefix("--> ") {
                let p: Vec<&str> = rest.splitn(3, ':').collect();
                if p.len() >= 2
                    && let Ok(row) = p[1].parse::<usize>()
                {
                    let col = p.get(2).and_then(|s| s.parse().ok()).unwrap_or(1);
                    location = Some((p[0].to_string(), row, col));
                }
                break;
            }
            if lines[j].starts_with("error") || lines[j].starts_with("warning") {
                break;
            }
            j += 1;
        }
        // Advance past this diagnostic's full block.
        let mut k = i + 1;
        while k < lines.len() {
            let next = lines[k];
            let is_new = !next.starts_with(' ')
                && !next.starts_with('\t')
                && (next.starts_with("error") || next.starts_with("warning"))
                && !next.trim().is_empty();
            if is_new {
                break;
            }
            k += 1;
        }
        let source_context = location.as_ref().and_then(|(rel, row, _)| {
            let abs = if Path::new(rel).is_absolute() {
                PathBuf::from(rel)
            } else {
                crate_root.join(rel)
            };
            let src = std::fs::read_to_string(&abs).ok()?;
            let src_lines: Vec<&str> = src.lines().collect();
            let center = row.saturating_sub(1);
            let start = center.saturating_sub(5);
            let end = (center + 6).min(src_lines.len());
            let snippet: Vec<String> = src_lines[start..end]
                .iter()
                .enumerate()
                .map(|(idx, l)| {
                    let lineno = start + idx + 1;
                    let marker = if lineno == *row { ">>>" } else { "   " };
                    format!("{marker} {lineno:4} | {l}")
                })
                .collect();
            Some(json!({ "file": rel, "line": row, "snippet": snippet.join("\n") }))
        });
        let mut diag = json!({ "level": level, "message": message });
        if let Some((f, r, c)) = &location {
            diag["file"] = json!(f);
            diag["line"] = json!(r);
            diag["col"] = json!(c);
        }
        if let Some(ctx) = source_context {
            diag["source_context"] = ctx;
        }
        diags.push(diag);
        i = k;
    }
    diags
}

// ── Generic diagnostic parsing (Go/Python/JS) ────────────────────────────────

fn parse_generic_diagnostics(output: &str, root: &Path) -> Vec<Value> {
    let re = regex_lite::Regex::new(r"(?m)^(.+?):(\d+)(?::(\d+))?:?\s*(.*)$").ok();
    let Some(re) = re else { return Vec::new() };

    let mut diags = Vec::new();
    let mut snippets: HashMap<(String, usize), String> = HashMap::new();
    for cap in re.captures_iter(output) {
        let file = cap.get(1).map(|m| m.as_str().to_string()).unwrap_or_default();
        let line = cap
            .get(2)
            .and_then(|m| m.as_str().parse::<usize>().ok())
            .unwrap_or(1);
        let col = cap
            .get(3)
            .and_then(|m| m.as_str().parse::<usize>().ok())
            .unwrap_or(1);
        let message = cap.get(4).map(|m| m.as_str().to_string()).unwrap_or_default();

        if file.is_empty() || message.is_empty() {
            continue;
        }
        let abs_path = if Path::new(&file).is_absolute() {
            PathBuf::from(&file)
        } else {
            root.join(&file)
        };
        if !abs_path.exists() {
            continue;
        }

        let level = if message.to_ascii_lowercase().contains("error") {
            "error"
        } else {
            "warning"
        };
        let mut diag = json!({
            "file": file,
            "line": line,
            "col": col,
            "message": message,
            "level": level,
        });

        let cache_key = (file.clone(), line);
        let snippet = if let Some(s) = snippets.get(&cache_key) {
            Some(s.clone())
        } else if let Ok(src) = std::fs::read_to_string(&abs_path) {
            let src_lines: Vec<&str> = src.lines().collect();
            let center = line.saturating_sub(1);
            if center < src_lines.len() {
                let start = center.saturating_sub(2);
                let end = (center + 3).min(src_lines.len());
                let s: Vec<String> = src_lines[start..end]
                    .iter()
                    .enumerate()
                    .map(|(idx, l)| {
                        let lineno = start + idx + 1;
                        let marker = if lineno == line { ">>>" } else { "   " };
                        format!("{marker} {lineno:4} | {l}")
                    })
                    .collect();
                let joined = s.join("\n");
                snippets.insert(cache_key, joined.clone());
                Some(joined)
            } else {
                None
            }
        } else {
            None
        };

        if let Some(s) = snippet {
            diag["source_context"] = json!({ "file": file, "line": line, "snippet": s });
        }
        diags.push(diag);
    }
    diags
}
