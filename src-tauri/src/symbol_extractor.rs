//! AST-aware symbol + import extraction using tree-sitter.
#![allow(dead_code)]
//!
//! Replaces the regex-based `extract_imports_from_source` in architect.rs.
//! Parses source files with tree-sitter to extract:
//!   - Import paths (for the dependency graph)
//!   - Symbol definitions (functions, classes, structs, etc.) for future use
//!
//! Parsers are cached per-thread (tree-sitter Parser is !Sync) so Rayon
//! parallel iteration gets one parser per worker thread without locking.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;
use tree_sitter::{Language, Node, Parser, Query, QueryCursor, StreamingIterator, Tree};

// ── Data model ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SymbolKind {
    Function,
    Method,
    Class,
    Interface,
    Trait,
    Struct,
    Enum,
    Module,
    Import,
    Route,
    Call,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Symbol {
    pub id: String,
    pub name: String,
    pub qualified_name: String,
    pub kind: SymbolKind,
    pub source_path: String,
    pub start_line: usize,
    pub end_line: usize,
}

// ── Language dispatch ──────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
pub enum Lang {
    Rust,
    TypeScript,
    Tsx,
    JavaScript,
    Python,
    Go,
}

impl Lang {
    /// Detect language from file extension. Returns None for unsupported files.
    pub fn detect(path: &Path) -> Option<Lang> {
        match path.extension()?.to_str()?.to_lowercase().as_str() {
            "rs" => Some(Lang::Rust),
            "ts" => Some(Lang::TypeScript),
            "tsx" => Some(Lang::Tsx),
            "js" | "jsx" | "mjs" | "cjs" => Some(Lang::JavaScript),
            "py" => Some(Lang::Python),
            "go" => Some(Lang::Go),
            _ => None,
        }
    }

    /// Get the tree-sitter Language for this language.
    fn ts_language(self) -> Language {
        match self {
            Lang::Rust => tree_sitter_rust::LANGUAGE.into(),
            Lang::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Lang::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
            Lang::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            Lang::Python => tree_sitter_python::LANGUAGE.into(),
            Lang::Go => tree_sitter_go::LANGUAGE.into(),
        }
    }
}

// ── Thread-local parser cache ─────────────────────────────────────
// tree-sitter Parser is !Sync, so we use thread_local to give each
// Rayon worker thread its own parser. The parser is reused across
// files of the same language within one thread.

thread_local! {
    static PARSER: std::cell::RefCell<Parser> = std::cell::RefCell::new(Parser::new());
}

/// Parse source code with the given language, using a thread-local parser.
/// Returns None if the language can't be set (ABI mismatch).
fn parse_source(lang: Lang, source: &str) -> Option<Tree> {
    PARSER.with(|p| {
        let mut parser = p.borrow_mut();
        parser.set_language(&lang.ts_language()).ok()?;
        parser.parse(source.as_bytes(), None)
    })
}

// ── Queries (compiled once per language via once_cell) ────────────

fn imports_query(lang: Lang) -> Option<&'static Query> {
    use once_cell::sync::Lazy;

    static TS_IMPORTS: Lazy<Option<Query>> = Lazy::new(|| {
        Query::new(
            &tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            r#"
(import_statement source: (string) @path) @import
(export_statement source: (string) @path) @import
(call_expression
  function: (identifier) @fn
  arguments: (arguments (string) @path)
  (#eq? @fn "require")) @import
(call_expression
  function: (import)
  arguments: (arguments (string) @path)) @import
"#,
        )
        .ok()
    });

    static PY_IMPORTS: Lazy<Option<Query>> = Lazy::new(|| {
        Query::new(
            &tree_sitter_python::LANGUAGE.into(),
            r#"
(import_statement name: (dotted_name) @module) @import
(import_from_statement module_name: (dotted_name) @module) @import
(import_from_statement module_name: (relative_import) @module) @import
"#,
        )
        .ok()
    });

    static GO_IMPORTS: Lazy<Option<Query>> = Lazy::new(|| {
        Query::new(
            &tree_sitter_go::LANGUAGE.into(),
            r#"
(import_spec path: (interpreted_string_literal) @path) @import
"#,
        )
        .ok()
    });

    let q = match lang {
        Lang::TypeScript | Lang::Tsx | Lang::JavaScript => &TS_IMPORTS,
        Lang::Python => &PY_IMPORTS,
        Lang::Go => &GO_IMPORTS,
        Lang::Rust => return None, // Rust uses AST walking, not queries
    };
    q.as_ref()
}

// ── Public API ─────────────────────────────────────────────────────

/// Extract import paths from a source file using tree-sitter.
///
/// Returns a `Vec<String>` of raw import specifiers (same interface as the
/// old `extract_imports_from_source`), suitable for passing to
/// `resolve_imported_files`.
///
/// If tree-sitter can't parse the file (unsupported language, ABI error),
/// returns an empty vec — the caller should fall back to regex if needed.
pub fn extract_imports(file_path: &str, content: &str) -> Vec<String> {
    let path = Path::new(file_path);
    let lang = match Lang::detect(path) {
        Some(l) => l,
        None => return Vec::new(),
    };

    let tree = match parse_source(lang, content) {
        Some(t) => t,
        None => {
            tracing::debug!("symbol_extractor: parse failed for {}", file_path);
            return Vec::new();
        }
    };

    match lang {
        Lang::Rust => extract_rust_imports(&tree, content.as_bytes()),
        _ => extract_query_imports(&tree, content.as_bytes(), lang),
    }
}

/// Extract symbol definitions from a source file using tree-sitter.
/// Returns symbols with stable IDs and line spans.
pub fn extract_symbols(project: &str, file_path: &str, content: &str) -> Vec<Symbol> {
    let path = Path::new(file_path);
    let lang = match Lang::detect(path) {
        Some(l) => l,
        None => return Vec::new(),
    };

    let tree = match parse_source(lang, content) {
        Some(t) => t,
        None => return Vec::new(),
    };

    let query = match symbols_query(lang) {
        Some(q) => q,
        None => return Vec::new(),
    };
    let mut cursor = QueryCursor::new();

    let name_idx = query.capture_index_for_name("name");
    let def_idx = query.capture_index_for_name("def");

    let mut matches = cursor.matches(query, tree.root_node(), content.as_bytes());
    let mut symbols = Vec::new();

    while let Some(m) = matches.next() {
        let mut name_node: Option<tree_sitter::Node> = None;
        let mut def_node: Option<tree_sitter::Node> = None;

        for cap in m.captures.iter() {
            if Some(cap.index) == name_idx {
                name_node = Some(cap.node);
            } else if Some(cap.index) == def_idx {
                def_node = Some(cap.node);
            }
        }

        let (Some(name_node), Some(def_node)) = (name_node, def_node) else {
            continue;
        };

        let name: String = name_node
            .utf8_text(content.as_bytes())
            .unwrap_or("")
            .to_string();
        let qualified = qualified_name(name_node, content.as_bytes(), &name);
        let kind = kind_from_node(def_node.kind());
        let (start_line, end_line) = node_lines(def_node);

        symbols.push(Symbol {
            id: stable_id(project, file_path, &qualified, &format!("{:?}", kind)),
            name,
            qualified_name: qualified,
            kind,
            source_path: file_path.to_string(),
            start_line,
            end_line,
        });
    }

    symbols
}

// ── Import extraction implementations ──────────────────────────────

fn extract_query_imports(tree: &Tree, source: &[u8], lang: Lang) -> Vec<String> {
    let query = match imports_query(lang) {
        Some(q) => q,
        None => return Vec::new(),
    };
    let mut cursor = QueryCursor::new();

    let path_idx = query.capture_index_for_name("path");
    let module_idx = query.capture_index_for_name("module");

    let mut matches = cursor.matches(query, tree.root_node(), source);
    let mut imports = Vec::new();

    while let Some(m) = matches.next() {
        for cap in m.captures.iter() {
            let is_path = path_idx.map(|i| i == cap.index).unwrap_or(false);
            let is_module = module_idx.map(|i| i == cap.index).unwrap_or(false);

            if is_path || is_module {
                let raw = cap.node.utf8_text(source).unwrap_or("").to_string();
                let normalized = normalize_import(&raw, lang);
                if let Some(n) = normalized {
                    // Filter empty strings — they're not valid import paths
                    if !n.is_empty() {
                        imports.push(n);
                    }
                }
            }
        }
    }

    imports
}

fn extract_rust_imports(tree: &Tree, source: &[u8]) -> Vec<String> {
    let mut imports = Vec::new();

    fn visit(node: Node, source: &[u8], imports: &mut Vec<String>) {
        match node.kind() {
            "use_declaration" => {
                if let Some(arg) = node.child_by_field_name("argument") {
                    let mut paths = Vec::new();
                    collect_rust_use_paths(arg, source, "", &mut paths);
                    for p in paths {
                        // Normalize: crate::foo::bar -> src/foo/bar
                        let normalized = p.replace("::", "/");
                        let normalized = if normalized.starts_with("crate/") {
                            format!("src/{}", &normalized["crate/".len()..])
                        } else if normalized.starts_with("self/") {
                            normalized["self/".len()..].to_string()
                        } else if normalized == "crate" || normalized == "self" || normalized == "super" {
                            continue; // skip self-references
                        } else {
                            // External crate (e.g. "tokio", "serde") — skip, resolve_imported_files handles it
                            normalized
                        };
                        if !normalized.is_empty() && !normalized.contains('*') {
                            imports.push(normalized);
                        }
                    }
                }
            }
            "mod_item" => {
                // `mod foo;` (without body) is an import-like reference.
                if node.child_by_field_name("body").is_none() {
                    if let Some(name) = node.child_by_field_name("name") {
                        if let Ok(text) = name.utf8_text(source) {
                            imports.push(text.to_string());
                        }
                    }
                }
            }
            _ => {}
        }

        for i in 0..node.child_count() {
            if let Some(child) = node.child(i) {
                visit(child, source, imports);
            }
        }
    }

    visit(tree.root_node(), source, &mut imports);
    imports
}

fn collect_rust_use_paths(node: Node, source: &[u8], prefix: &str, out: &mut Vec<String>) {
    match node.kind() {
        "identifier" | "scoped_identifier" | "crate" | "super" | "self" | "use_wildcard" => {
            if let Ok(t) = node.utf8_text(source) {
                let t = t.trim();
                if !t.is_empty() && t != "*" {
                    out.push(format!("{}{}", prefix, t));
                }
            }
        }
        "use_as_clause" => {
            if let Some(path) = node.child_by_field_name("path") {
                collect_rust_use_paths(path, source, prefix, out);
            }
        }
        "use_list" => {
            for i in 0..node.named_child_count() {
                if let Some(child) = node.named_child(i) {
                    collect_rust_use_paths(child, source, prefix, out);
                }
            }
        }
        "scoped_use_list" => {
            let new_prefix = node
                .child_by_field_name("path")
                .and_then(|n| n.utf8_text(source).ok())
                .map(|t| format!("{}{}::", prefix, t.trim()))
                .unwrap_or_else(|| prefix.to_string());

            for i in 0..node.named_child_count() {
                if let Some(child) = node.named_child(i) {
                    collect_rust_use_paths(child, source, &new_prefix, out);
                }
            }
        }
        "use_tree" => {
            for i in 0..node.named_child_count() {
                if let Some(child) = node.named_child(i) {
                    collect_rust_use_paths(child, source, prefix, out);
                }
            }
        }
        _ => {
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i) {
                    collect_rust_use_paths(child, source, prefix, out);
                }
            }
        }
    }
}

// ── Import normalization ────────────────────────────────────────────

/// Normalize a raw captured import string into a path-like specifier
/// that `resolve_imported_files` can match against known files.
/// Returns None for empty/whitespace-only strings.
fn normalize_import(raw: &str, lang: Lang) -> Option<String> {
    match lang {
        Lang::TypeScript | Lang::Tsx | Lang::JavaScript | Lang::Go => {
            // Strip surrounding quotes (string literals)
            let s = raw.trim();
            let stripped = s
                .strip_prefix('"')
                .or_else(|| s.strip_prefix('\''))
                .and_then(|s| {
                    s.strip_suffix('"').or_else(|| s.strip_suffix('\''))
                });
            let result = stripped.unwrap_or(s).trim();
            if result.is_empty() { None } else { Some(result.to_string()) }
        }
        Lang::Python => {
            // "foo.bar.baz" -> "foo/bar/baz"
            let result = raw.trim().replace('.', "/");
            if result.is_empty() { None } else { Some(result) }
        }
        Lang::Rust => {
            // Already normalized in extract_rust_imports
            let result = raw.trim();
            if result.is_empty() { None } else { Some(result.to_string()) }
        }
    }
}

// ── Symbol queries ────────────────────────────────────────────────

fn symbols_query(lang: Lang) -> Option<&'static Query> {
    use once_cell::sync::Lazy;

    static RUST_SYMS: Lazy<Option<Query>> = Lazy::new(|| {
        Query::new(
            &tree_sitter_rust::LANGUAGE.into(),
            r#"
(function_item            name: (identifier)     @name) @def
(function_signature_item  name: (identifier)     @name) @def
(struct_item              name: (type_identifier) @name) @def
(enum_item                name: (type_identifier) @name) @def
(trait_item               name: (type_identifier) @name) @def
(union_item               name: (type_identifier) @name) @def
(type_item                name: (type_identifier) @name) @def
(mod_item                 name: (identifier)     @name) @def
(macro_definition         name: (identifier)     @name) @def
"#,
        )
        .ok()
    });

    static TS_SYMS: Lazy<Option<Query>> = Lazy::new(|| {
        Query::new(
            &tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            r#"
(function_declaration      name: (_) @name) @def
(class_declaration         name: (_) @name) @def
(method_definition         name: (_) @name) @def
(interface_declaration     name: (type_identifier) @name) @def
(type_alias_declaration    name: (type_identifier) @name) @def
(enum_declaration          name: (identifier) @name) @def
(abstract_class_declaration name: (type_identifier) @name) @def
"#,
        )
        .ok()
    });

    static PY_SYMS: Lazy<Option<Query>> = Lazy::new(|| {
        Query::new(
            &tree_sitter_python::LANGUAGE.into(),
            r#"
(function_definition name: (identifier) @name) @def
(class_definition    name: (identifier) @name) @def
"#,
        )
        .ok()
    });

    static GO_SYMS: Lazy<Option<Query>> = Lazy::new(|| {
        Query::new(
            &tree_sitter_go::LANGUAGE.into(),
            r#"
(function_declaration name: (identifier)      @name) @def
(method_declaration    name: (field_identifier) @name) @def
(type_declaration (type_spec name: (type_identifier) @name)) @def
"#,
        )
        .ok()
    });

    let q = match lang {
        Lang::Rust => &RUST_SYMS,
        Lang::TypeScript | Lang::Tsx | Lang::JavaScript => &TS_SYMS,
        Lang::Python => &PY_SYMS,
        Lang::Go => &GO_SYMS,
    };
    q.as_ref()
}

// ── Helpers ────────────────────────────────────────────────────────

fn qualified_name(mut name_node: Node, source: &[u8], fallback: &str) -> String {
    let mut parts = Vec::new();

    if let Ok(t) = name_node.utf8_text(source) {
        parts.push(t.to_string());
    } else {
        parts.push(fallback.to_string());
    }

    // Walk up to find enclosing class/module names
    while let Some(parent) = name_node.parent() {
        if let Some(parent_name) = parent.child_by_field_name("name") {
            if let Ok(t) = parent_name.utf8_text(source) {
                // Only prepend if it's a class/struct/impl/module, not a function
                let kind = parent.kind();
                if matches!(
                    kind,
                    "class_declaration"
                        | "class_definition"
                        | "struct_item"
                        | "impl_item"
                        | "mod_item"
                        | "trait_item"
                        | "interface_declaration"
                ) {
                    parts.push(t.to_string());
                }
            }
        }
        name_node = parent;
    }

    parts.reverse();
    parts.join("::")
}

fn node_lines(node: Node) -> (usize, usize) {
    let start = node.start_position();
    let end = node.end_position();

    let start_line = start.row + 1; // 1-based
    let end_line = if end.column == 0 {
        end.row // exclusive end at column 0 -> previous line
    } else {
        end.row + 1
    };

    (start_line, end_line)
}

fn stable_id(project: &str, rel_path: &str, qualified: &str, kind: &str) -> String {
    let payload = format!("{}|{}|{}|{}", project, rel_path, kind, qualified);
    format!("{:x}", Sha256::digest(payload.as_bytes()))
}

fn kind_from_node(kind: &str) -> SymbolKind {
    match kind {
        "function_item"
        | "function_declaration"
        | "function_definition"
        | "function_signature_item" => SymbolKind::Function,
        "method_declaration" | "method_definition" => SymbolKind::Method,
        "class_declaration" | "class_definition" | "struct_item" | "type_declaration" => {
            SymbolKind::Class
        }
        "interface_declaration" | "trait_item" => SymbolKind::Trait,
        "enum_item" | "enum_declaration" => SymbolKind::Enum,
        "mod_item" => SymbolKind::Module,
        "import_statement" | "use_declaration" | "import_declaration" => SymbolKind::Import,
        _ => SymbolKind::Unknown,
    }
}

// ── Tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_ts_imports() {
        let source = r#"
import { foo } from "./utils";
import bar from "../lib/bar";
const baz = require("lib/baz");
export type { Quux } from "./types";
"#;
        let imports = extract_imports("test.ts", source);
        assert!(imports.contains(&"./utils".to_string()));
        assert!(imports.contains(&"../lib/bar".to_string()));
        assert!(imports.contains(&"lib/baz".to_string()));
        assert!(imports.contains(&"./types".to_string()));
    }

    #[test]
    fn test_extract_rust_imports() {
        let source = r#"
use crate::architect::Phase1Response;
use std::collections::HashMap;
use serde::{Serialize, Deserialize};
mod sidebar;
"#;
        let imports = extract_imports("test.rs", source);
        assert!(imports.contains(&"src/architect/Phase1Response".to_string()));
        assert!(imports.contains(&"sidebar".to_string()));
        // External crates like std::collections::HashMap should be present
        // (resolve_imported_files will skip them since they're not local)
        assert!(imports.iter().any(|i| i.contains("collections/HashMap")));
    }

    #[test]
    fn test_extract_python_imports() {
        let source = r#"
import os
from foo.bar import baz
from . import utils
"#;
        let imports = extract_imports("test.py", source);
        assert!(imports.contains(&"os".to_string()));
        assert!(imports.contains(&"foo/bar".to_string()));
    }

    #[test]
    fn test_extract_go_imports() {
        let source = r#"
package main

import (
    "fmt"
    "github.com/foo/bar"
)
"#;
        let imports = extract_imports("test.go", source);
        assert!(imports.contains(&"fmt".to_string()));
        assert!(imports.contains(&"github.com/foo/bar".to_string()));
    }

    #[test]
    fn test_extract_symbols_rust() {
        let source = r#"
struct Foo { x: i32 }
fn main() {}
fn helper() {}
"#;
        let symbols = extract_symbols("test/repo", "src/main.rs", source);
        assert!(symbols.iter().any(|s| s.name == "Foo" && s.kind == SymbolKind::Class));
        assert!(symbols.iter().any(|s| s.name == "main" && s.kind == SymbolKind::Function));
        assert!(symbols.iter().any(|s| s.name == "helper" && s.kind == SymbolKind::Function));
    }

    #[test]
    fn test_extract_symbols_ts() {
        let source = r#"
function add(a: number, b: number): number { return a + b; }
class Calculator { compute() {} }
interface ICalc { run(): void; }
"#;
        let symbols = extract_symbols("test/repo", "src/calc.ts", source);
        assert!(symbols.iter().any(|s| s.name == "add" && s.kind == SymbolKind::Function));
        assert!(symbols.iter().any(|s| s.name == "Calculator" && s.kind == SymbolKind::Class));
        assert!(symbols.iter().any(|s| s.name == "ICalc" && s.kind == SymbolKind::Trait));
    }

    #[test]
    fn test_stable_id_consistency() {
        let id1 = stable_id("owner/repo", "src/main.rs", "main", "Function");
        let id2 = stable_id("owner/repo", "src/main.rs", "main", "Function");
        assert_eq!(id1, id2, "same inputs should produce same ID");

        let id3 = stable_id("owner/repo", "src/main.rs", "helper", "Function");
        assert_ne!(id1, id3, "different names should produce different IDs");
    }

    #[test]
    fn test_lang_detect() {
        assert!(matches!(Lang::detect(Path::new("foo.rs")), Some(Lang::Rust)));
        assert!(matches!(Lang::detect(Path::new("foo.ts")), Some(Lang::TypeScript)));
        assert!(matches!(Lang::detect(Path::new("foo.tsx")), Some(Lang::Tsx)));
        assert!(matches!(Lang::detect(Path::new("foo.js")), Some(Lang::JavaScript)));
        assert!(matches!(Lang::detect(Path::new("foo.py")), Some(Lang::Python)));
        assert!(matches!(Lang::detect(Path::new("foo.go")), Some(Lang::Go)));
        assert!(Lang::detect(Path::new("foo.txt")).is_none());
        assert!(Lang::detect(Path::new("foo.md")).is_none());
    }
}
