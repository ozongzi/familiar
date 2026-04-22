use agentix::tool;
use serde_json::{Value, json};
use tree_sitter::{Node, Parser};

// ── constants ──────────────────────────────────────────────────────────────────

/// Lines in a node before we fold its children instead of returning full source.
const FOLD_THRESHOLD: usize = 80;

// ── signature matching ─────────────────────────────────────────────────────────

/// Normalize whitespace for fuzzy signature matching.
fn normalize(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Extract the "signature" of a node — the text up to but not including its body block.
/// For items without a body (use, type alias, const, static), returns the full text.
fn node_signature<'a>(node: Node<'a>, src: &'a str) -> &'a str {
    // Body is the last child named "block" or "declaration_list"
    let body_start = (0..node.child_count()).rev().find_map(|i| {
        let child = node.child(i as u32)?;
        if child.kind() == "block" || child.kind() == "declaration_list" {
            Some(child.start_byte())
        } else {
            None
        }
    });
    let end = body_start.unwrap_or(node.end_byte());
    src[node.start_byte()..end].trim_end()
}

/// Whether this node kind is a "block-level" item that can appear in a scope path.
fn is_block_item(kind: &str) -> bool {
    matches!(
        kind,
        "function_item"
            | "impl_item"
            | "mod_item"
            | "trait_item"
            | "struct_item"
            | "enum_item"
            | "attribute_item"
    )
}

/// Match a scope segment against a node.
/// The segment is matched against the normalized signature of the node.
/// For attribute_item, match against the attribute text (e.g. "#[cfg(test)]").
fn segment_matches(node: Node, src: &str, segment: &str) -> bool {
    let seg = normalize(segment);
    let sig = normalize(node_signature(node, src));
    sig.contains(&seg) || seg.contains(&sig)
}

/// Find a node matching `segment` among direct named children of `parent`.
/// If segment starts with "#[", also search attribute_item nodes and return the
/// *following sibling* (the item the attribute is attached to).
fn find_child_matching<'a>(parent: Node<'a>, src: &'a str, segment: &str) -> Option<Node<'a>> {
    let is_attr = segment.trim_start().starts_with("#[");

    let mut cursor = parent.walk();
    let children: Vec<Node> = parent.named_children(&mut cursor).collect();

    if is_attr {
        // Find the attribute_item that matches, then return the next sibling
        for (i, child) in children.iter().enumerate() {
            if child.kind() == "attribute_item" && segment_matches(*child, src, segment) {
                return children.get(i + 1).copied();
            }
        }
        return None;
    }

    for child in &children {
        if is_block_item(child.kind()) && segment_matches(*child, src, segment) {
            return Some(*child);
        }
    }
    None
}

/// Resolve a scope path to a node. Returns the node and whether it was found.
/// Empty scope = root (source_file node).
fn resolve_scope<'a>(root: Node<'a>, src: &'a str, scope: &[String]) -> Result<Node<'a>, String> {
    let mut current = root;
    for (i, segment) in scope.iter().enumerate() {
        match find_child_matching(current, src, segment) {
            Some(node) => current = node,
            None => {
                return Err(format!(
                    "scope[{}] {:?} not found under {}",
                    i,
                    segment,
                    if i == 0 {
                        "file root".to_string()
                    } else {
                        format!("scope[{}]", i - 1)
                    }
                ));
            }
        }
    }
    Ok(current)
}

// ── BFS folded read ────────────────────────────────────────────────────────────

/// Return direct block-item children of a node.
fn block_children(node: Node, src: &str) -> Vec<(String, usize)> {
    // Get the body container (declaration_list or block), or the node itself for root
    let container = (0..node.child_count())
        .find_map(|i| {
            let child = node.child(i as u32)?;
            if child.kind() == "declaration_list" || child.kind() == "block" {
                Some(child)
            } else {
                None
            }
        })
        .unwrap_or(node);

    let mut cursor = container.walk();
    container
        .named_children(&mut cursor)
        .filter(|c| is_block_item(c.kind()))
        .map(|c| {
            let sig = node_signature(c, src).to_string();
            let lines = src[c.start_byte()..c.end_byte()].lines().count();
            (sig, lines)
        })
        .collect()
}

/// Render a node's source with BFS folding:
/// - If the node's total lines <= FOLD_THRESHOLD: return full source
/// - Otherwise: show the signature, then fold each direct block child's body
fn render_node(node: Node, src: &str) -> String {
    let node_src = &src[node.start_byte()..node.end_byte()];
    let line_count = node_src.lines().count();

    if line_count <= FOLD_THRESHOLD {
        return node_src.to_string();
    }

    // Get body container
    let container = (0..node.child_count()).find_map(|i| {
        let child = node.child(i as u32)?;
        if child.kind() == "declaration_list" || child.kind() == "block" {
            Some(child)
        } else {
            None
        }
    });

    let Some(container) = container else {
        // No body — just return as-is even if large
        return node_src.to_string();
    };

    // Signature part (before the body)
    let sig_end = container.start_byte();
    let sig = &src[node.start_byte()..sig_end];

    // Build folded body
    let mut result = sig.to_string();
    result.push('{');

    let mut cursor = container.walk();
    for child in container.named_children(&mut cursor) {
        if is_block_item(child.kind()) {
            let child_src = &src[child.start_byte()..child.end_byte()];
            let child_lines = child_src.lines().count();
            let child_sig = node_signature(child, src);

            // Check if this child itself has a body
            let has_body = (0..child.child_count()).any(|i| {
                child
                    .child(i as u32)
                    .map(|c| c.kind() == "block" || c.kind() == "declaration_list")
                    .unwrap_or(false)
            });

            result.push('\n');
            if has_body && child_lines > 5 {
                result.push_str(&format!(
                    "    {} {{ /* {} lines */ }}",
                    child_sig.trim(),
                    child_lines
                ));
            } else {
                // Small enough — show fully
                for line in child_src.lines() {
                    result.push_str(&format!("\n    {}", line));
                }
            }
        }
    }

    result.push_str("\n}");
    result
}

// ── outline ────────────────────────────────────────────────────────────────────

fn outline_node(node: Node, src: &str) -> Vec<Value> {
    block_children(node, src)
        .into_iter()
        .map(|(sig, lines)| json!({ "signature": sig.trim(), "lines": lines }))
        .collect()
}

// ── edit helpers ───────────────────────────────────────────────────────────────

/// Get the body container's byte range (start of `{`, end of `}`).
/// For source_file, returns the full file range.
fn body_range(node: Node) -> (usize, usize) {
    let container = (0..node.child_count()).find_map(|i| {
        let child = node.child(i as u32)?;
        if child.kind() == "declaration_list" || child.kind() == "block" {
            Some(child)
        } else {
            None
        }
    });
    match container {
        Some(c) => (c.start_byte(), c.end_byte()),
        None => (node.start_byte(), node.end_byte()),
    }
}

/// Insert `new_source` inside the end of a node's body.
fn insert_inside_end(src: &str, node: Node, new_source: &str) -> String {
    let (body_start, body_end) = body_range(node);
    // body_end points at the closing `}` — insert before it
    let closing_brace = body_end.saturating_sub(1);

    // Figure out indentation from existing children
    let indent = detect_body_indent(src, body_start, body_end);

    let mut result = src[..closing_brace].to_string();
    // Ensure newline before insertion
    if !result.ends_with('\n') {
        result.push('\n');
    }
    for line in new_source.lines() {
        if line.trim().is_empty() {
            result.push('\n');
        } else {
            result.push_str(&indent);
            result.push_str(line);
            result.push('\n');
        }
    }
    result.push_str(&src[closing_brace..]);
    result
}

/// Detect indentation used inside a body block.
fn detect_body_indent(src: &str, body_start: usize, body_end: usize) -> String {
    let body = &src[body_start..body_end];
    for line in body.lines().skip(1) {
        let trimmed = line.trim_start();
        if !trimmed.is_empty() {
            let spaces = line.len() - trimmed.len();
            return " ".repeat(spaces);
        }
    }
    "    ".to_string()
}

/// Append `new_source` after `node` at the top level (for root-level inserts).
fn insert_after_node(src: &str, node: Node, new_source: &str) -> String {
    let end = node.end_byte();
    let mut result = src[..end].to_string();
    result.push_str("\n\n");
    result.push_str(new_source.trim_end());
    result.push('\n');
    result.push_str(&src[end..]);
    result
}

// ── parse helper ──────────────────────────────────────────────────────────────

fn parse_rust(src: &str) -> Option<tree_sitter::Tree> {
    let mut parser = Parser::new();
    let lang = tree_sitter_rust::LANGUAGE;
    parser.set_language(&lang.into()).ok()?;
    parser.parse(src, None)
}

// ── AstSpell ──────────────────────────────────────────────────────────────────

pub struct AstSpell;

#[tool]
impl agentix::Tool for AstSpell {
    /// Read a Rust source file, navigating by block scope.
    ///
    /// Returns the node's source with BFS folding: nodes under 80 lines are
    /// shown in full; larger nodes show each direct child folded to a one-liner.
    /// Use `outline` operation to list child signatures without content.
    ///
    /// path: absolute path to the .rs file
    /// scope: block signature path from outermost to innermost,
    ///        e.g. ["impl Foo for Bar", "fn send"].
    ///        Empty = file root.
    /// operation: "read" (default) or "outline"
    async fn rust_read(
        &self,
        path: String,
        scope: Vec<String>,
        operation: Option<String>,
    ) -> Value {
        let src = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => return json!({ "error": format!("read failed: {e}") }),
        };
        let tree = match parse_rust(&src) {
            Some(t) => t,
            None => return json!({ "error": "failed to parse file" }),
        };
        let root = tree.root_node();

        let node = match resolve_scope(root, &src, &scope) {
            Ok(n) => n,
            Err(e) => return json!({ "error": e }),
        };

        match operation.as_deref().unwrap_or("read") {
            "outline" => {
                let items = outline_node(node, &src);
                json!({ "path": path, "scope": scope, "children": items })
            }
            _ => {
                let content = render_node(node, &src);
                let lines = content.lines().count();
                let folded = content.lines().count()
                    < src[node.start_byte()..node.end_byte()].lines().count();
                json!({
                    "path": path,
                    "scope": scope,
                    "lines": lines,
                    "folded": folded,
                    "content": content,
                })
            }
        }
    }

    /// Replace the entire source of a scoped block with `new_source`.
    ///
    /// The node identified by `scope` (including its signature) is replaced wholesale.
    /// For the file root (empty scope), replaces the entire file.
    ///
    /// path: absolute path to the .rs file
    /// scope: path to the block to replace (empty = whole file)
    /// new_source: replacement source text
    async fn rust_replace(&self, path: String, scope: Vec<String>, new_source: String) -> Value {
        let src = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => return json!({ "error": format!("read failed: {e}") }),
        };

        if scope.is_empty() {
            if let Err(e) = std::fs::write(&path, &new_source) {
                return json!({ "error": format!("write failed: {e}") });
            }
            return json!({ "replaced": path, "scope": scope });
        }

        let tree = match parse_rust(&src) {
            Some(t) => t,
            None => return json!({ "error": "failed to parse file" }),
        };
        let root = tree.root_node();
        let node = match resolve_scope(root, &src, &scope) {
            Ok(n) => n,
            Err(e) => return json!({ "error": e }),
        };

        let mut result = src[..node.start_byte()].to_string();
        result.push_str(new_source.trim_end());
        result.push('\n');
        result.push_str(&src[node.end_byte()..]);

        if let Err(e) = std::fs::write(&path, &result) {
            return json!({ "error": format!("write failed: {e}") });
        }
        json!({ "replaced": path, "scope": scope })
    }

    /// Insert `new_source` inside the end of a scoped block's body,
    /// or after the node if `position` is "after".
    /// For empty scope (file root), appends to end of file.
    ///
    /// path: absolute path to the .rs file
    /// scope: path to the containing block (empty = file root)
    /// new_source: source text to insert
    /// position: "inside_end" (default) or "after"
    async fn rust_insert(
        &self,
        path: String,
        scope: Vec<String>,
        new_source: String,
        position: Option<String>,
    ) -> Value {
        let src = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => return json!({ "error": format!("read failed: {e}") }),
        };

        let pos = position.as_deref().unwrap_or("inside_end");

        let result = if scope.is_empty() {
            // File root: append to end
            let mut r = src.trim_end().to_string();
            r.push_str("\n\n");
            r.push_str(new_source.trim_end());
            r.push('\n');
            r
        } else {
            let tree = match parse_rust(&src) {
                Some(t) => t,
                None => return json!({ "error": "failed to parse file" }),
            };
            let root = tree.root_node();
            let node = match resolve_scope(root, &src, &scope) {
                Ok(n) => n,
                Err(e) => return json!({ "error": e }),
            };

            match pos {
                "after" => insert_after_node(&src, node, &new_source),
                _ => insert_inside_end(&src, node, &new_source),
            }
        };

        if let Err(e) = std::fs::write(&path, &result) {
            return json!({ "error": format!("write failed: {e}") });
        }
        json!({ "inserted": path, "scope": scope, "position": pos })
    }

    /// Delete the block identified by `scope`.
    ///
    /// path: absolute path to the .rs file
    /// scope: path to the block to delete (must be non-empty)
    async fn rust_delete(&self, path: String, scope: Vec<String>) -> Value {
        if scope.is_empty() {
            return json!({ "error": "scope must not be empty for delete" });
        }
        let src = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => return json!({ "error": format!("read failed: {e}") }),
        };
        let tree = match parse_rust(&src) {
            Some(t) => t,
            None => return json!({ "error": "failed to parse file" }),
        };
        let root = tree.root_node();
        let node = match resolve_scope(root, &src, &scope) {
            Ok(n) => n,
            Err(e) => return json!({ "error": e }),
        };

        // Delete the node and any immediately preceding blank line
        let start = node.start_byte();
        let end = node.end_byte();

        // Trim preceding blank line if present
        let trim_start = {
            let before = &src[..start];
            let trimmed = before.trim_end_matches('\n');
            // Keep one newline as separator from previous item
            trimmed.len() + 1
        };

        let mut result = src[..trim_start.min(start)].to_string();
        result.push_str(&src[end..]);

        if let Err(e) = std::fs::write(&path, &result) {
            return json!({ "error": format!("write failed: {e}") });
        }
        json!({ "deleted": path, "scope": scope })
    }
}
