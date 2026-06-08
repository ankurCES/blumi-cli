//! Structural code extraction via tree-sitter (Tier-1, feature `code-graph`).
//!
//! Parses a source file into **declarations** (with kind / fully-qualified name /
//! enclosing parent / signature) plus the raw **reference sites** (calls, type
//! references, trait impls) and **imports**. The P2 resolver turns sites into
//! typed, resolved edges. Rust first; more grammars fan out in P6.
//!
//! Compiled only under `--features code-graph`, so the default build stays
//! native-lite (no C grammars linked).

use tree_sitter::{Node, Parser};

const MAX_SNIPPET_LINES: usize = 40;

/// The relation a reference site (or resolved edge) represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeKind {
    /// `src` calls `dst`.
    Call,
    /// `src` references `dst` as a type.
    Type,
    /// `src` (a type) implements trait `dst`.
    Implements,
}

impl EdgeKind {
    /// The `code_edges.kind` string for this relation.
    pub fn as_str(self) -> &'static str {
        match self {
            EdgeKind::Call => "call",
            EdgeKind::Type => "type",
            EdgeKind::Implements => "implements",
        }
    }
}

/// A declaration — the unit of retrieval: fn / struct / enum / trait / impl / …
#[derive(Debug, Clone)]
pub struct Decl {
    pub name: String,
    pub kind: String,
    /// Scope-qualified name, e.g. `Engine::run`.
    pub fqname: String,
    /// Enclosing declaration's fqname, if any (for `parent_id` / `contains`).
    pub parent: Option<String>,
    pub start_line: usize,
    pub end_line: usize,
    /// First line of the declaration (for display).
    pub signature: String,
    pub snippet: String,
}

/// An unresolved reference from a declaration to a name — resolved into a typed
/// edge by the P2 resolver.
#[derive(Debug, Clone)]
pub struct Site {
    pub from_fqname: String,
    pub name: String,
    pub kind: EdgeKind,
    pub line: usize,
}

/// An `import` / `use` binding (best-effort): the bound name + its full path.
#[derive(Debug, Clone)]
pub struct Import {
    pub name: String,
    pub path: String,
}

/// Everything one parsed file contributes to the graph.
#[derive(Debug, Default)]
pub struct Parsed {
    pub decls: Vec<Decl>,
    pub sites: Vec<Site>,
    pub imports: Vec<Import>,
}

/// Parse `content` structurally for `lang`. Returns `None` when no grammar is
/// bundled for the language (the caller falls back to the regex extractor).
pub fn extract_structural(_path: &str, content: &str, lang: &str) -> Option<Parsed> {
    let language: tree_sitter::Language = match lang {
        "rust" => tree_sitter_rust::LANGUAGE.into(),
        _ => return None,
    };
    let mut parser = Parser::new();
    parser.set_language(&language).ok()?;
    let tree = parser.parse(content, None)?;
    let src = content.as_bytes();
    let mut out = Parsed::default();
    let mut stack: Vec<String> = Vec::new();
    walk(tree.root_node(), src, &mut stack, &mut out);
    Some(out)
}

fn decl_kind(node_kind: &str) -> Option<&'static str> {
    Some(match node_kind {
        "function_item" => "fn",
        "struct_item" | "union_item" => "struct",
        "enum_item" => "enum",
        "trait_item" => "trait",
        "mod_item" => "mod",
        "type_item" => "type",
        "const_item" | "static_item" => "const",
        "macro_definition" => "macro",
        "impl_item" => "impl",
        _ => return None,
    })
}

fn walk(node: Node, src: &[u8], stack: &mut Vec<String>, out: &mut Parsed) {
    let kind = node.kind();
    let mut pushed = false;

    if let Some(dk) = decl_kind(kind) {
        if let Some(name) = decl_name(node, src, dk) {
            let parent = stack.last().cloned();
            let fqname = match &parent {
                Some(p) => format!("{p}::{name}"),
                None => name.clone(),
            };
            let start_line = node.start_position().row + 1;
            let end_line = node.end_position().row + 1;
            let snippet = node_snippet(node, src);
            let signature = snippet
                .lines()
                .next()
                .unwrap_or("")
                .trim_end_matches('{')
                .trim()
                .to_string();
            // `impl Trait for Type` → an Implements site from the type.
            if dk == "impl" {
                if let Some(tr) = node.child_by_field_name("trait") {
                    if let Some(trait_name) = last_ident(tr, src) {
                        out.sites.push(Site {
                            from_fqname: fqname.clone(),
                            name: trait_name,
                            kind: EdgeKind::Implements,
                            line: start_line,
                        });
                    }
                }
            }
            out.decls.push(Decl {
                name,
                kind: dk.to_string(),
                fqname: fqname.clone(),
                parent,
                start_line,
                end_line,
                signature,
                snippet,
            });
            stack.push(fqname);
            pushed = true;
        }
    } else if kind == "call_expression" {
        if let (Some(from), Some(name)) = (stack.last().cloned(), call_name(node, src)) {
            out.sites.push(Site {
                from_fqname: from,
                name,
                kind: EdgeKind::Call,
                line: node.start_position().row + 1,
            });
        }
    } else if kind == "type_identifier" && !is_decl_or_impl_name(node) {
        if let (Some(from), Some(name)) = (stack.last().cloned(), text(node, src)) {
            out.sites.push(Site {
                from_fqname: from,
                name,
                kind: EdgeKind::Type,
                line: node.start_position().row + 1,
            });
        }
    } else if kind == "use_declaration" {
        if let Some(arg) = node.child_by_field_name("argument") {
            if let Some(name) = last_ident(arg, src) {
                out.imports.push(Import {
                    name,
                    path: text(arg, src).unwrap_or_default(),
                });
            }
        }
    }

    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();
    for child in children {
        walk(child, src, stack, out);
    }
    if pushed {
        stack.pop();
    }
}

fn decl_name(node: Node, src: &[u8], kind: &str) -> Option<String> {
    if kind == "impl" {
        // `impl Type` / `impl Trait for Type` → the implementing type's name.
        return node
            .child_by_field_name("type")
            .and_then(|t| last_ident(t, src));
    }
    node.child_by_field_name("name").and_then(|n| text(n, src))
}

fn call_name(call: Node, src: &[u8]) -> Option<String> {
    let f = call.child_by_field_name("function")?;
    match f.kind() {
        "identifier" => text(f, src),
        "field_expression" => f.child_by_field_name("field").and_then(|n| text(n, src)),
        "scoped_identifier" => f
            .child_by_field_name("name")
            .and_then(|n| text(n, src))
            .or_else(|| last_ident(f, src)),
        _ => last_ident(f, src),
    }
}

/// True when this `type_identifier` is the *name* of a declaration (or the
/// type/trait of an impl) rather than a type *reference* — those are emitted by
/// the declaration handling, not as `Type` sites.
fn is_decl_or_impl_name(n: Node) -> bool {
    let Some(p) = n.parent() else { return false };
    match p.kind() {
        "struct_item" | "enum_item" | "union_item" | "trait_item" | "type_item" => p
            .child_by_field_name("name")
            .map(|x| x.id() == n.id())
            .unwrap_or(false),
        "impl_item" => true,
        _ => false,
    }
}

fn last_ident(n: Node, src: &[u8]) -> Option<String> {
    if matches!(
        n.kind(),
        "identifier" | "type_identifier" | "field_identifier"
    ) {
        return text(n, src);
    }
    let mut cursor = n.walk();
    let children: Vec<Node> = n.children(&mut cursor).collect();
    for child in children.into_iter().rev() {
        if let Some(s) = last_ident(child, src) {
            return Some(s);
        }
    }
    None
}

fn text(n: Node, src: &[u8]) -> Option<String> {
    n.utf8_text(src).ok().map(|s| s.to_string())
}

fn node_snippet(n: Node, src: &[u8]) -> String {
    n.utf8_text(src)
        .unwrap_or("")
        .lines()
        .take(MAX_SNIPPET_LINES)
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
use std::collections::HashMap;

pub struct Engine {
    cache: HashMap<String, u32>,
}

impl Engine {
    pub fn new() -> Self {
        Engine { cache: HashMap::new() }
    }
    pub fn run(&self) -> bool {
        self.check()
    }
    fn check(&self) -> bool {
        true
    }
}

pub trait Runner {
    fn go(&self);
}

impl Runner for Engine {
    fn go(&self) {
        let _ = self.run();
    }
}
"#;

    #[test]
    fn rust_extraction_decls_sites_imports() {
        let p = extract_structural("engine.rs", SAMPLE, "rust").expect("rust grammar");

        // Declarations carry kind + fully-qualified name + parent + signature.
        assert!(p
            .decls
            .iter()
            .any(|d| d.kind == "struct" && d.name == "Engine"));
        assert!(p
            .decls
            .iter()
            .any(|d| d.kind == "trait" && d.name == "Runner"));
        let run = p
            .decls
            .iter()
            .find(|d| d.fqname == "Engine::run")
            .expect("method fqname is scope-qualified");
        assert_eq!(run.kind, "fn");
        assert_eq!(run.parent.as_deref(), Some("Engine"));
        assert!(
            run.signature.contains("fn run"),
            "signature: {}",
            run.signature
        );

        // Call sites are anchored to their enclosing declaration.
        assert!(
            p.sites.iter().any(|s| s.kind == EdgeKind::Call
                && s.name == "check"
                && s.from_fqname == "Engine::run"),
            "expected Engine::run -> check call site"
        );
        // `impl Runner for Engine` → an Implements site.
        assert!(p
            .sites
            .iter()
            .any(|s| s.kind == EdgeKind::Implements && s.name == "Runner"));
        // Type references are captured (e.g. HashMap in the field type).
        assert!(p
            .sites
            .iter()
            .any(|s| s.kind == EdgeKind::Type && s.name == "HashMap"));
        // Imports are captured best-effort.
        assert!(p.imports.iter().any(|i| i.name == "HashMap"));
    }

    #[test]
    fn unknown_language_is_none() {
        assert!(extract_structural("x.txt", "plain text", "").is_none());
    }
}
