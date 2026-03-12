use std::time::Duration;

use ds_api::tool;
use glob::glob as glob_walk;
use serde_json::{Value, json};
use tokio::fs;
use tokio::process::Command;
use tree_sitter::{Language, Node, Parser};

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
    async fn search(
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
                    if let Some(file) = cur_file.take() {
                        if !cur_lines.is_empty() {
                            matches.push(json!({ "file": file, "lines": cur_lines.clone() }));
                        }
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
    async fn outline(&self, description: Option<String>, path: String) -> Value {
        let _ = description;
        let content = match fs::read_to_string(&path).await {
            Ok(s) => s,
            Err(e) => return json!({ "error": e.to_string() }),
        };
        let total = content.lines().count();
        super::outline_value(&path, &content, total)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tree-sitter outline helpers (shared with file_spells via super::outline_value)
// ─────────────────────────────────────────────────────────────────────────────

pub(crate) fn outline_value(path: &str, content: &str, total_lines: usize) -> Value {
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    let Some(language) = lang_for_ext(&ext) else {
        return json!({
            "path": path,
            "total_lines": total_lines,
            "note": format!("不支持 .{ext} 的符号提取，请用 read(from, to) 按段读取"),
        });
    };

    let src = content.as_bytes();
    let mut parser = Parser::new();
    if parser.set_language(&language).is_err() {
        return json!({ "error": "tree-sitter 语言初始化失败" });
    }
    let Some(tree) = parser.parse(src, None) else {
        return json!({ "error": "解析失败（文件可能为空或编码异常）" });
    };

    let symbols = extract_symbols(tree.root_node(), src, &ext);
    let items: Vec<Value> = symbols
        .iter()
        .map(|s| {
            json!({
                "kind": s.kind, "name": s.name, "line": s.line, "end_line": s.end_line,
            })
        })
        .collect();

    json!({
        "path": path,
        "language": ext,
        "total_lines": total_lines,
        "symbols": items,
        "symbol_count": items.len(),
        "hint": "文件较大，已返回符号大纲。用 read(path, from, to) 读取具体段落。",
    })
}

fn lang_for_ext(ext: &str) -> Option<Language> {
    match ext {
        "rs" => Some(tree_sitter_rust::LANGUAGE.into()),
        "py" | "pyw" => Some(tree_sitter_python::LANGUAGE.into()),
        "js" | "mjs" | "cjs" => Some(tree_sitter_javascript::LANGUAGE.into()),
        "ts" | "mts" | "cts" => Some(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()),
        "tsx" => Some(tree_sitter_typescript::LANGUAGE_TSX.into()),
        "go" => Some(tree_sitter_go::LANGUAGE.into()),
        "java" => Some(tree_sitter_java::LANGUAGE.into()),
        "c" | "h" => Some(tree_sitter_c::LANGUAGE.into()),
        "cpp" | "cc" | "cxx" | "hpp" | "hxx" | "hh" => Some(tree_sitter_cpp::LANGUAGE.into()),
        "toml" => Some(tree_sitter_toml_ng::LANGUAGE.into()),
        _ => None,
    }
}

struct Symbol {
    kind: &'static str,
    name: String,
    line: usize,
    end_line: usize,
}

fn extract_symbols(root: Node, src: &[u8], ext: &str) -> Vec<Symbol> {
    match ext {
        "rs" => walk(
            root,
            src,
            &[
                ("function_item", "fn", Some("name")),
                ("impl_item", "impl", None),
                ("struct_item", "struct", Some("name")),
                ("enum_item", "enum", Some("name")),
                ("trait_item", "trait", Some("name")),
                ("type_item", "type", Some("name")),
                ("const_item", "const", Some("name")),
                ("mod_item", "mod", Some("name")),
                ("macro_definition", "macro", Some("name")),
            ],
        ),
        "py" | "pyw" => walk(
            root,
            src,
            &[
                ("function_definition", "def", Some("name")),
                ("async_function_definition", "async def", Some("name")),
                ("class_definition", "class", Some("name")),
            ],
        ),
        "js" | "mjs" | "cjs" => walk(
            root,
            src,
            &[
                ("function_declaration", "function", Some("name")),
                ("class_declaration", "class", Some("name")),
                ("method_definition", "method", Some("name")),
            ],
        ),
        "ts" | "mts" | "cts" | "tsx" => walk(
            root,
            src,
            &[
                ("function_declaration", "function", Some("name")),
                ("class_declaration", "class", Some("name")),
                ("interface_declaration", "interface", Some("name")),
                ("type_alias_declaration", "type", Some("name")),
                ("enum_declaration", "enum", Some("name")),
                ("method_definition", "method", Some("name")),
            ],
        ),
        "go" => walk(
            root,
            src,
            &[
                ("function_declaration", "func", Some("name")),
                ("method_declaration", "method", Some("name")),
                ("type_declaration", "type", None),
            ],
        ),
        "java" => walk(
            root,
            src,
            &[
                ("class_declaration", "class", Some("name")),
                ("interface_declaration", "interface", Some("name")),
                ("method_declaration", "method", Some("name")),
                ("constructor_declaration", "constructor", Some("name")),
            ],
        ),
        "c" | "h" => walk(
            root,
            src,
            &[
                ("function_definition", "function", Some("declarator")),
                ("struct_specifier", "struct", Some("name")),
                ("enum_specifier", "enum", Some("name")),
            ],
        ),
        "cpp" | "cc" | "cxx" | "hpp" | "hxx" | "hh" => walk(
            root,
            src,
            &[
                ("function_definition", "function", Some("declarator")),
                ("class_specifier", "class", Some("name")),
                ("namespace_definition", "namespace", Some("name")),
            ],
        ),
        "toml" => walk(
            root,
            src,
            &[
                ("table", "table", None),
                ("table_array_element", "[[table]]", None),
            ],
        ),
        _ => vec![],
    }
}

fn walk(
    root: Node,
    src: &[u8],
    rules: &[(&'static str, &'static str, Option<&'static str>)],
) -> Vec<Symbol> {
    let mut out = vec![];
    let mut cursor = root.walk();
    walk_rec(root, src, rules, &mut out, &mut cursor);
    out
}

fn walk_rec(
    node: Node,
    src: &[u8],
    rules: &[(&'static str, &'static str, Option<&'static str>)],
    out: &mut Vec<Symbol>,
    cursor: &mut tree_sitter::TreeCursor,
) {
    for &(node_kind, label, name_field) in rules {
        if node.kind() == node_kind {
            let name = match name_field {
                Some(f) => node
                    .child_by_field_name(f)
                    .and_then(|n| n.utf8_text(src).ok())
                    .unwrap_or("<anonymous>")
                    .to_string(),
                None => node
                    .utf8_text(src)
                    .unwrap_or("<anonymous>")
                    .chars()
                    .take(80)
                    .collect(),
            };
            out.push(Symbol {
                kind: label,
                name,
                line: node.start_position().row + 1,
                end_line: node.end_position().row + 1,
            });
        }
    }
    if cursor.goto_first_child() {
        loop {
            walk_rec(cursor.node(), src, rules, out, cursor);
            if !cursor.goto_next_sibling() {
                break;
            }
        }
        cursor.goto_parent();
    }
}
