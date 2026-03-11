use ds_api::tool;
use serde_json::{Value, json};
use tree_sitter::{Language, Node, Parser};

pub struct OutlineSpell;

// ── Language detection ────────────────────────────────────────────────────────

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

// ── Symbol kinds ──────────────────────────────────────────────────────────────

#[derive(Debug)]
struct Symbol {
    kind: &'static str,
    name: String,
    /// 1-based start line
    line: usize,
    /// 1-based end line
    end_line: usize,
}

// ── Per-language extractors ───────────────────────────────────────────────────

fn extract_symbols(root: Node, src: &[u8], ext: &str) -> Vec<Symbol> {
    match ext {
        "rs" => extract_rust(root, src),
        "py" | "pyw" => extract_python(root, src),
        "js" | "mjs" | "cjs" => extract_javascript(root, src),
        "ts" | "mts" | "cts" => extract_typescript(root, src),
        "tsx" => extract_typescript(root, src),
        "go" => extract_go(root, src),
        "java" => extract_java(root, src),
        "c" | "h" => extract_c(root, src),
        "cpp" | "cc" | "cxx" | "hpp" | "hxx" | "hh" => extract_cpp(root, src),
        "toml" => extract_toml(root, src),
        _ => vec![],
    }
}

// ── Generic tree walker ───────────────────────────────────────────────────────

/// Walk the tree and collect nodes matching `kinds`, extracting the child node
/// named `name_field` as the symbol name. If `name_field` is None the node's
/// own text is used directly.
fn walk_collect(
    root: Node,
    src: &[u8],
    rules: &[(&'static str, &'static str, Option<&'static str>)],
    // (node_kind, symbol_kind_label, name_child_field)
) -> Vec<Symbol> {
    let mut out = Vec::new();
    let mut cursor = root.walk();
    walk_recursive(root, src, rules, &mut out, &mut cursor);
    out
}

fn walk_recursive(
    node: Node,
    src: &[u8],
    rules: &[(&'static str, &'static str, Option<&'static str>)],
    out: &mut Vec<Symbol>,
    cursor: &mut tree_sitter::TreeCursor,
) {
    for (node_kind, label, name_field) in rules {
        if node.kind() == *node_kind {
            let name = if let Some(field) = name_field {
                node.child_by_field_name(field)
                    .and_then(|n| n.utf8_text(src).ok())
                    .unwrap_or("<anonymous>")
                    .to_string()
            } else {
                node.utf8_text(src).unwrap_or("<anonymous>").to_string()
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
            walk_recursive(cursor.node(), src, rules, out, cursor);
            if !cursor.goto_next_sibling() {
                break;
            }
        }
        cursor.goto_parent();
    }
}

// ── Rust ──────────────────────────────────────────────────────────────────────

fn extract_rust(root: Node, src: &[u8]) -> Vec<Symbol> {
    walk_collect(
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
            ("static_item", "static", Some("name")),
            ("mod_item", "mod", Some("name")),
            ("macro_definition", "macro", Some("name")),
        ],
    )
}

// ── Python ────────────────────────────────────────────────────────────────────

fn extract_python(root: Node, src: &[u8]) -> Vec<Symbol> {
    walk_collect(
        root,
        src,
        &[
            ("function_definition", "def", Some("name")),
            ("async_function_definition", "async def", Some("name")),
            ("class_definition", "class", Some("name")),
            ("decorated_definition", "decorated", None),
        ],
    )
}

// ── JavaScript ───────────────────────────────────────────────────────────────

fn extract_javascript(root: Node, src: &[u8]) -> Vec<Symbol> {
    walk_collect(
        root,
        src,
        &[
            ("function_declaration", "function", Some("name")),
            ("generator_function_declaration", "function*", Some("name")),
            ("class_declaration", "class", Some("name")),
            ("method_definition", "method", Some("name")),
            ("arrow_function", "arrow fn", None),
            ("lexical_declaration", "const/let", None),
        ],
    )
}

// ── TypeScript ────────────────────────────────────────────────────────────────

fn extract_typescript(root: Node, src: &[u8]) -> Vec<Symbol> {
    walk_collect(
        root,
        src,
        &[
            ("function_declaration", "function", Some("name")),
            ("generator_function_declaration", "function*", Some("name")),
            ("class_declaration", "class", Some("name")),
            ("interface_declaration", "interface", Some("name")),
            ("type_alias_declaration", "type", Some("name")),
            ("enum_declaration", "enum", Some("name")),
            ("method_definition", "method", Some("name")),
            ("abstract_class_declaration", "abstract class", Some("name")),
            ("module", "namespace", Some("name")),
        ],
    )
}

// ── Go ────────────────────────────────────────────────────────────────────────

fn extract_go(root: Node, src: &[u8]) -> Vec<Symbol> {
    walk_collect(
        root,
        src,
        &[
            ("function_declaration", "func", Some("name")),
            ("method_declaration", "method", Some("name")),
            ("type_declaration", "type", None),
            ("const_declaration", "const", None),
            ("var_declaration", "var", None),
        ],
    )
}

// ── Java ──────────────────────────────────────────────────────────────────────

fn extract_java(root: Node, src: &[u8]) -> Vec<Symbol> {
    walk_collect(
        root,
        src,
        &[
            ("class_declaration", "class", Some("name")),
            ("interface_declaration", "interface", Some("name")),
            ("enum_declaration", "enum", Some("name")),
            ("annotation_type_declaration", "@interface", Some("name")),
            ("method_declaration", "method", Some("name")),
            ("constructor_declaration", "constructor", Some("name")),
            ("record_declaration", "record", Some("name")),
        ],
    )
}

// ── C ─────────────────────────────────────────────────────────────────────────

fn extract_c(root: Node, src: &[u8]) -> Vec<Symbol> {
    walk_collect(
        root,
        src,
        &[
            ("function_definition", "function", Some("declarator")),
            ("struct_specifier", "struct", Some("name")),
            ("union_specifier", "union", Some("name")),
            ("enum_specifier", "enum", Some("name")),
            ("type_definition", "typedef", None),
            ("preproc_def", "#define", Some("name")),
            ("preproc_function_def", "#define fn", Some("name")),
        ],
    )
}

// ── C++ ───────────────────────────────────────────────────────────────────────

fn extract_cpp(root: Node, src: &[u8]) -> Vec<Symbol> {
    walk_collect(
        root,
        src,
        &[
            ("function_definition", "function", Some("declarator")),
            ("class_specifier", "class", Some("name")),
            ("struct_specifier", "struct", Some("name")),
            ("namespace_definition", "namespace", Some("name")),
            ("template_declaration", "template", None),
            ("enum_specifier", "enum", Some("name")),
            ("type_definition", "typedef", None),
        ],
    )
}

// ── TOML ──────────────────────────────────────────────────────────────────────

fn extract_toml(root: Node, src: &[u8]) -> Vec<Symbol> {
    // TOML: top-level tables and array-of-tables are the useful structural markers
    walk_collect(
        root,
        src,
        &[
            ("table", "table", None),
            ("table_array_element", "[[table]]", None),
        ],
    )
}

// ── Tool implementation ───────────────────────────────────────────────────────

#[tool]
impl Tool for OutlineSpell {
    /// 提取源文件的符号大纲（函数、类、结构体、接口等），返回每个符号的名称、类型和行号范围。
    /// 适合在读取大文件前快速了解结构，或定位某个符号所在的行范围后再用 get 精确读取。
    ///
    /// 支持语言：Rust、Python、JavaScript、TypeScript、TSX、Go、Java、C、C++、TOML
    ///
    /// path: 源文件路径
    async fn outline(&self, path: String) -> Value {
        // ── Detect language from extension ────────────────────────────────────
        let ext = std::path::Path::new(&path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        let language = match lang_for_ext(&ext) {
            Some(l) => l,
            None => {
                return json!({
                    "error": format!(
                        "不支持的文件类型 '.{ext}'。支持：rs, py, js, ts, tsx, go, java, c, h, cpp, cc, cxx, hpp, toml"
                    )
                });
            }
        };

        // ── Read source ───────────────────────────────────────────────────────
        let src = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => return json!({ "error": e.to_string() }),
        };

        let total_lines = src.iter().filter(|&&b| b == b'\n').count() + 1;

        // ── Parse ─────────────────────────────────────────────────────────────
        let mut parser = Parser::new();
        if let Err(e) = parser.set_language(&language) {
            return json!({ "error": format!("设置语言失败: {e}") });
        }

        let tree = match parser.parse(&src, None) {
            Some(t) => t,
            None => return json!({ "error": "解析失败（源文件可能为空或编码异常）" }),
        };

        // ── Extract symbols ───────────────────────────────────────────────────
        let symbols = extract_symbols(tree.root_node(), &src, &ext);

        let items: Vec<Value> = symbols
            .iter()
            .map(|s| {
                json!({
                    "kind": s.kind,
                    "name": s.name,
                    "line": s.line,
                    "end_line": s.end_line,
                })
            })
            .collect();

        json!({
            "path": path,
            "language": ext,
            "total_lines": total_lines,
            "symbols": items,
            "symbol_count": items.len(),
        })
    }
}
