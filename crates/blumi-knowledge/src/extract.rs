//! Lightweight, dependency-free symbol extraction.
//!
//! A per-language set of line-anchored regexes finds top-level declarations
//! (functions, classes, structs, …). A symbol's body runs from its declaration
//! line to just before the next declaration (capped), and the snippet is the
//! first ~40 lines of that body. Files in unknown languages (or with no matched
//! declarations) fall back to fixed-size chunks so they're still searchable.
//!
//! This is intentionally "native-lite": no tree-sitter / C grammars. It trades a
//! little boundary precision for a single small binary that builds anywhere —
//! and the [`crate::KnowledgeStore`] API is unchanged if a precise parser is
//! swapped in later.

use regex::Regex;
use std::sync::OnceLock;

/// An extracted code symbol.
#[derive(Debug, Clone)]
pub struct Symbol {
    pub name: String,
    pub kind: String,
    pub start_line: usize, // 1-based
    pub end_line: usize,
    pub snippet: String,
}

const MAX_SNIPPET_LINES: usize = 40;
const CHUNK_LINES: usize = 50;

/// Map a file extension to a coarse language id (used for rule selection + the
/// `lang` column). Empty string = unknown (chunk-only).
pub fn lang_for(path: &str) -> &'static str {
    let ext = path.rsplit('.').next().unwrap_or("").to_lowercase();
    match ext.as_str() {
        "rs" => "rust",
        "py" => "python",
        "js" | "jsx" | "mjs" | "cjs" => "javascript",
        "ts" | "tsx" => "typescript",
        "dart" => "dart",
        "go" => "go",
        "java" => "java",
        "kt" | "kts" => "kotlin",
        "rb" => "ruby",
        "c" | "h" => "c",
        "cc" | "cpp" | "cxx" | "hpp" => "cpp",
        "swift" => "swift",
        _ => "",
    }
}

struct Rule {
    re: Regex,
    kind: &'static str,
}

fn rules_for(lang: &str) -> &'static [Rule] {
    static RUST: OnceLock<Vec<Rule>> = OnceLock::new();
    static PY: OnceLock<Vec<Rule>> = OnceLock::new();
    static JSTS: OnceLock<Vec<Rule>> = OnceLock::new();
    static DART: OnceLock<Vec<Rule>> = OnceLock::new();
    static GO: OnceLock<Vec<Rule>> = OnceLock::new();
    static GENERIC: OnceLock<Vec<Rule>> = OnceLock::new();

    fn r(pat: &str, kind: &'static str) -> Rule {
        Rule {
            re: Regex::new(pat).expect("valid symbol regex"),
            kind,
        }
    }

    match lang {
        "rust" => RUST.get_or_init(|| {
            vec![
                r(r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?(?:unsafe\s+)?(?:const\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)", "fn"),
                r(r"^\s*(?:pub(?:\([^)]*\))?\s+)?struct\s+([A-Za-z_][A-Za-z0-9_]*)", "struct"),
                r(r"^\s*(?:pub(?:\([^)]*\))?\s+)?enum\s+([A-Za-z_][A-Za-z0-9_]*)", "enum"),
                r(r"^\s*(?:pub(?:\([^)]*\))?\s+)?trait\s+([A-Za-z_][A-Za-z0-9_]*)", "trait"),
                r(r"^\s*impl(?:<[^>]*>)?\s+(?:[A-Za-z0-9_:<>, &']+\s+for\s+)?([A-Za-z_][A-Za-z0-9_]*)", "impl"),
                r(r"^\s*macro_rules!\s+([A-Za-z_][A-Za-z0-9_]*)", "macro"),
            ]
        }),
        "python" => PY.get_or_init(|| {
            vec![
                r(r"^\s*(?:async\s+)?def\s+([A-Za-z_][A-Za-z0-9_]*)", "fn"),
                r(r"^\s*class\s+([A-Za-z_][A-Za-z0-9_]*)", "class"),
            ]
        }),
        "javascript" | "typescript" => JSTS.get_or_init(|| {
            vec![
                r(r"^\s*(?:export\s+)?(?:default\s+)?(?:async\s+)?function\s*\*?\s*([A-Za-z_$][A-Za-z0-9_$]*)", "fn"),
                r(r"^\s*(?:export\s+)?(?:abstract\s+)?class\s+([A-Za-z_$][A-Za-z0-9_$]*)", "class"),
                r(r"^\s*(?:export\s+)?interface\s+([A-Za-z_$][A-Za-z0-9_$]*)", "interface"),
                r(r"^\s*(?:export\s+)?type\s+([A-Za-z_$][A-Za-z0-9_$]*)\s*=", "type"),
                r(r"^\s*(?:export\s+)?(?:const|let|var)\s+([A-Za-z_$][A-Za-z0-9_$]*)\s*=\s*(?:async\s*)?\(?[^=]*=>", "fn"),
            ]
        }),
        "dart" => DART.get_or_init(|| {
            vec![
                r(r"^\s*(?:abstract\s+)?class\s+([A-Za-z_][A-Za-z0-9_]*)", "class"),
                r(r"^\s*(?:mixin|enum|extension)\s+([A-Za-z_][A-Za-z0-9_]*)", "type"),
            ]
        }),
        "go" => GO.get_or_init(|| {
            vec![
                r(r"^\s*func\s+(?:\([^)]*\)\s*)?([A-Za-z_][A-Za-z0-9_]*)", "fn"),
                r(r"^\s*type\s+([A-Za-z_][A-Za-z0-9_]*)\s+struct", "struct"),
                r(r"^\s*type\s+([A-Za-z_][A-Za-z0-9_]*)\s+interface", "interface"),
            ]
        }),
        // C / C++ / Java / Kotlin / Swift / Ruby — a permissive shared set.
        _ => GENERIC.get_or_init(|| {
            vec![
                r(r"^\s*(?:public\s+|private\s+|protected\s+|static\s+|final\s+|abstract\s+)*class\s+([A-Za-z_][A-Za-z0-9_]*)", "class"),
                r(r"^\s*(?:func|def|fun)\s+([A-Za-z_][A-Za-z0-9_]*)", "fn"),
            ]
        }),
    }
}

/// Extract symbols from `content`. Always returns at least one entry for a
/// non-empty file (chunk fallback), so every file is searchable.
pub fn extract(path: &str, content: &str) -> Vec<Symbol> {
    let lang = lang_for(path);
    let lines: Vec<&str> = content.lines().collect();
    if lines.is_empty() {
        return Vec::new();
    }

    // Pass 1: find declaration start lines (1-based) + name + kind.
    let rules = if lang.is_empty() {
        &[][..]
    } else {
        rules_for(lang)
    };
    let mut starts: Vec<(usize, String, &'static str)> = Vec::new();
    if !rules.is_empty() {
        for (i, line) in lines.iter().enumerate() {
            // Skip obvious comment lines to cut false positives.
            let t = line.trim_start();
            if t.starts_with("//") || t.starts_with('#') || t.starts_with('*') {
                continue;
            }
            for rule in rules {
                if let Some(c) = rule.re.captures(line) {
                    if let Some(m) = c.get(1) {
                        starts.push((i + 1, m.as_str().to_string(), rule.kind));
                        break;
                    }
                }
            }
        }
    }

    // No declarations found → chunk the file so it's still searchable.
    if starts.is_empty() {
        return chunk(path, &lines);
    }

    // Pass 2: each symbol's body runs to just before the next declaration.
    let mut out = Vec::with_capacity(starts.len());
    for (idx, (start, name, kind)) in starts.iter().enumerate() {
        let next_start = starts
            .get(idx + 1)
            .map(|(s, _, _)| *s)
            .unwrap_or(lines.len() + 1);
        let end = (next_start - 1).max(*start);
        let snip_end = (*start + MAX_SNIPPET_LINES - 1).min(end);
        let snippet = lines[(start - 1)..snip_end].join("\n");
        out.push(Symbol {
            name: name.clone(),
            kind: kind.to_string(),
            start_line: *start,
            end_line: end,
            snippet,
        });
    }
    out
}

/// Fixed-size chunks for files we can't parse, named `<basename>:L<start>`.
fn chunk(path: &str, lines: &[&str]) -> Vec<Symbol> {
    let base = path.rsplit('/').next().unwrap_or(path);
    let mut out = Vec::new();
    let mut start = 0;
    while start < lines.len() {
        let end = (start + CHUNK_LINES).min(lines.len());
        let snippet = lines[start..end].join("\n");
        if !snippet.trim().is_empty() {
            out.push(Symbol {
                name: format!("{base}:L{}", start + 1),
                kind: "chunk".to_string(),
                start_line: start + 1,
                end_line: end,
                snippet,
            });
        }
        start = end;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_rust_symbols() {
        let src = "pub struct Foo {\n  a: u32,\n}\n\npub fn bar(x: u32) -> u32 {\n  x + 1\n}\n\nimpl Foo {\n  fn baz(&self) {}\n}\n";
        let syms = extract("src/lib.rs", src);
        let names: Vec<&str> = syms.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"Foo"));
        assert!(names.contains(&"bar"));
        // The `bar` snippet should include its body, not bleed into `impl`.
        let bar = syms.iter().find(|s| s.name == "bar").unwrap();
        assert!(bar.snippet.contains("x + 1"));
        assert!(!bar.snippet.contains("impl Foo"));
    }

    #[test]
    fn extracts_python_symbols() {
        let src = "import os\n\nclass Cat:\n    def meow(self):\n        return 'mew'\n\ndef free_fn():\n    pass\n";
        let syms = extract("a.py", src);
        let names: Vec<&str> = syms.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"Cat"));
        assert!(names.contains(&"meow"));
        assert!(names.contains(&"free_fn"));
    }

    #[test]
    fn unknown_lang_falls_back_to_chunks() {
        let src = (0..120)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let syms = extract("notes.txt", &src);
        assert!(syms.len() >= 2);
        assert!(syms.iter().all(|s| s.kind == "chunk"));
    }
}
