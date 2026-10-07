//! Tree-sitter-backed AST extraction for source files.
//!
//! Covers JavaScript / TypeScript / Python / Go. Each parsed
//! file produces a [`ParsedFile`] containing the declarations the
//! chunker (Phase 3 step 3) and the prompt assembler (Phase 4) need to
//! reason about: functions, classes, imports, exports.
//!
//! Per ADR-0001 + rules.md §12.3: chunk boundaries follow these
//! declarations rather than arbitrary token splits, so the AST
//! extractor's accuracy here directly determines how clean RAG hits
//! look downstream.

use serde::{Deserialize, Serialize};
use tree_sitter::{Node, Parser, Tree};

use crate::error::{AppError, AppResult};
use crate::services::file_discovery_service::SourceLanguage;

/// One declaration extracted from a source file. The kind is preserved
/// so chunk metadata can carry it forward into the vector index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeclarationKind {
    Function,
    Method,
    Class,
}

/// Single function / method / class declaration with byte-range
/// boundaries usable by the chunker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Declaration {
    pub kind: DeclarationKind,
    /// Identifier name. Empty for anonymous declarations (e.g. arrow
    /// functions assigned to const).
    pub name: String,
    /// 1-based start line (inclusive).
    pub start_line: u32,
    /// 1-based end line (inclusive).
    pub end_line: u32,
    /// 0-based byte offset of the declaration start.
    pub start_byte: u32,
    /// 0-based byte offset of the declaration end (exclusive).
    pub end_byte: u32,
}

/// Module-level import (any `import` form).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Import {
    /// Module path / specifier (e.g. `"react"`, `"./util"`,
    /// `"os.path"`).
    pub source: String,
    /// 1-based line where the import appears.
    pub line: u32,
}

/// Module-level export (any `export` form). Phase 3 captures the
/// statement boundary; expanding to per-name detail comes with the
/// dependency-graph work in Phase 5.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Export {
    /// Optional name when easily extractable; empty otherwise.
    pub name: String,
    pub line: u32,
}

/// Full parse result for one source file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ParsedFile {
    pub language: Option<SourceLanguage>,
    pub declarations: Vec<Declaration>,
    pub imports: Vec<Import>,
    pub exports: Vec<Export>,
    /// Source-file errors recorded by Tree-sitter (`ERROR` nodes).
    /// Non-fatal — partial AST results are still useful for chunking.
    pub error_count: u32,
}

/// Parse `source` according to the `language` grammar and return the
/// extracted declarations / imports / exports.
///
/// # Errors
///
/// - [`AppError::InvalidInput`] when the language is `Unknown` (no
///   grammar available in Phase 3).
/// - [`AppError::Internal`] when the Tree-sitter parser cannot be
///   configured for the requested grammar (e.g. ABI mismatch, which
///   indicates a build-time problem rather than user input).
pub fn parse(source: &str, language: SourceLanguage) -> AppResult<ParsedFile> {
    let grammar = match language {
        SourceLanguage::JavaScript => tree_sitter_javascript::language(),
        SourceLanguage::TypeScript => tree_sitter_typescript::language_typescript(),
        SourceLanguage::Python => tree_sitter_python::language(),
        SourceLanguage::Go => tree_sitter_go::language(),
        SourceLanguage::Unknown => {
            return Err(AppError::InvalidInput(
                "no Tree-sitter grammar available for Unknown language".into(),
            ))
        }
    };

    let mut parser = Parser::new();
    parser
        .set_language(&grammar)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("set_language failed: {e}")))?;

    let Some(tree) = parser.parse(source, None) else {
        // tree-sitter returns None only on cancellation or hard failure.
        // For a well-formed grammar this should not occur for any
        // bounded source string.
        return Ok(ParsedFile {
            language: Some(language),
            ..Default::default()
        });
    };

    let mut parsed = ParsedFile {
        language: Some(language),
        ..Default::default()
    };
    walk(&tree, source, language, &mut parsed);
    Ok(parsed)
}

/// Walk the syntax tree once, populating the parsed-file buckets in a
/// single pass.
fn walk(tree: &Tree, source: &str, language: SourceLanguage, out: &mut ParsedFile) {
    let mut error_count: u32 = 0;
    visit_node(tree.root_node(), source, language, out, &mut error_count);
    out.error_count = error_count;
}

fn visit_node(
    node: Node<'_>,
    source: &str,
    language: SourceLanguage,
    out: &mut ParsedFile,
    error_count: &mut u32,
) {
    if node.is_error() {
        *error_count = error_count.saturating_add(1);
    }

    classify_node(node, source, language, out);

    // Recurse into every child. Earlier versions of this function tried
    // to opt-in only structural containers, but the kind list grew
    // brittle (e.g. `class_declaration` -> `class_body` -> `method_definition`
    // requires walking through the class node, not skipping it). Tree-
    // sitter ASTs for typical source files are small enough that the
    // extra walk is negligible compared to the parse step itself.
    let mut child = node.walk();
    for c in node.children(&mut child) {
        visit_node(c, source, language, out, error_count);
    }
}

fn classify_node(node: Node<'_>, source: &str, language: SourceLanguage, out: &mut ParsedFile) {
    match language {
        SourceLanguage::JavaScript | SourceLanguage::TypeScript => {
            classify_js_ts(node, source, out);
        }
        SourceLanguage::Python => classify_python(node, source, out),
        SourceLanguage::Go => classify_go(node, source, out),
        SourceLanguage::Unknown => {}
    }
}

fn classify_js_ts(node: Node<'_>, source: &str, out: &mut ParsedFile) {
    match node.kind() {
        "function_declaration" => {
            push_declaration(node, source, DeclarationKind::Function, "name", out);
        }
        "method_definition" => {
            push_declaration(node, source, DeclarationKind::Method, "name", out);
        }
        "class_declaration" => {
            push_declaration(node, source, DeclarationKind::Class, "name", out);
        }
        "import_statement" => {
            if let Some(spec) = first_string_literal(node, source) {
                out.imports.push(Import {
                    source: spec,
                    line: line_of(node),
                });
            }
        }
        "export_statement" => {
            out.exports.push(Export {
                name: extract_export_name(node, source),
                line: line_of(node),
            });
        }
        _ => {}
    }
}

fn classify_python(node: Node<'_>, source: &str, out: &mut ParsedFile) {
    match node.kind() {
        "function_definition" => {
            push_declaration(node, source, DeclarationKind::Function, "name", out);
        }
        "class_definition" => {
            push_declaration(node, source, DeclarationKind::Class, "name", out);
        }
        "import_statement" | "import_from_statement" => {
            let module = python_import_module(node, source);
            out.imports.push(Import {
                source: module,
                line: line_of(node),
            });
        }
        _ => {}
    }
}

fn classify_go(node: Node<'_>, source: &str, out: &mut ParsedFile) {
    match node.kind() {
        "function_declaration" => {
            push_declaration(node, source, DeclarationKind::Function, "name", out);
        }
        "method_declaration" => {
            push_declaration(node, source, DeclarationKind::Method, "name", out);
        }
        // `type Foo struct { ... }`. Tree-sitter-go nests the name +
        // type under a `type_spec`; tag structs as `Class` so downstream
        // chunking treats them like the aggregate types they are
        // (mirrors how Python classes are tagged). Non-struct type specs
        // (interfaces, aliases) are skipped — Phase 3 only chunks
        // function/method/struct boundaries.
        "type_spec"
            if node
                .child_by_field_name("type")
                .is_some_and(|t| t.kind() == "struct_type") =>
        {
            push_declaration(node, source, DeclarationKind::Class, "name", out);
        }
        // Matches both the single `import "fmt"` form and each spec
        // inside a grouped `import ( ... )` block, since the walk
        // recurses through `import_spec_list`.
        "import_spec" => {
            if let Some(text) = node
                .child_by_field_name("path")
                .and_then(|p| p.utf8_text(source.as_bytes()).ok())
            {
                out.imports.push(Import {
                    source: text.trim_matches('"').to_string(),
                    line: line_of(node),
                });
            }
        }
        _ => {}
    }
}

fn push_declaration(
    node: Node<'_>,
    source: &str,
    kind: DeclarationKind,
    name_field: &str,
    out: &mut ParsedFile,
) {
    let name = node
        .child_by_field_name(name_field)
        .and_then(|n| n.utf8_text(source.as_bytes()).ok())
        .unwrap_or_default()
        .to_string();
    let start = node.start_position();
    let end = node.end_position();
    out.declarations.push(Declaration {
        kind,
        name,
        start_line: u32::try_from(start.row)
            .unwrap_or(u32::MAX)
            .saturating_add(1),
        end_line: u32::try_from(end.row).unwrap_or(u32::MAX).saturating_add(1),
        start_byte: u32::try_from(node.start_byte()).unwrap_or(u32::MAX),
        end_byte: u32::try_from(node.end_byte()).unwrap_or(u32::MAX),
    });
}

fn line_of(node: Node<'_>) -> u32 {
    u32::try_from(node.start_position().row)
        .unwrap_or(u32::MAX)
        .saturating_add(1)
}

fn first_string_literal(node: Node<'_>, source: &str) -> Option<String> {
    let mut cursor = node.walk();
    for c in node.children(&mut cursor) {
        if c.kind() == "string" {
            // Strip surrounding quotes.
            let text = c.utf8_text(source.as_bytes()).ok()?;
            return Some(text.trim_matches(|ch| ch == '"' || ch == '\'').to_string());
        }
    }
    None
}

fn extract_export_name(node: Node<'_>, source: &str) -> String {
    let mut cursor = node.walk();
    for c in node.children(&mut cursor) {
        match c.kind() {
            "function_declaration" | "class_declaration" | "lexical_declaration" => {
                if let Some(name_node) = c.child_by_field_name("name") {
                    if let Ok(name) = name_node.utf8_text(source.as_bytes()) {
                        return name.to_string();
                    }
                }
            }
            "identifier" => {
                if let Ok(name) = c.utf8_text(source.as_bytes()) {
                    return name.to_string();
                }
            }
            _ => {}
        }
    }
    String::new()
}

fn python_import_module(node: Node<'_>, source: &str) -> String {
    // For both `import x` and `from x import y`, the module identifier
    // sits as a child node. Walk and return the first dotted_name /
    // identifier child.
    let mut cursor = node.walk();
    for c in node.children(&mut cursor) {
        if matches!(c.kind(), "dotted_name" | "identifier") {
            if let Ok(text) = c.utf8_text(source.as_bytes()) {
                return text.to_string();
            }
        }
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_language_returns_invalid_input() {
        let err = parse("anything", SourceLanguage::Unknown).expect_err("must reject");
        assert_eq!(err.code(), "INVALID_INPUT");
    }

    #[test]
    fn typescript_extracts_function_declaration() {
        let source = r"
export function add(a: number, b: number): number {
    return a + b;
}
";
        let parsed = parse(source, SourceLanguage::TypeScript).expect("parse");
        assert_eq!(parsed.declarations.len(), 1);
        let d = &parsed.declarations[0];
        assert_eq!(d.kind, DeclarationKind::Function);
        assert_eq!(d.name, "add");
        assert!(d.end_byte > d.start_byte);
        assert_eq!(parsed.exports.len(), 1);
    }

    #[test]
    fn typescript_extracts_class_with_methods() {
        let source = r"
class Counter {
    count: number = 0;
    increment(): void {
        this.count += 1;
    }
    reset(): void {
        this.count = 0;
    }
}
";
        let parsed = parse(source, SourceLanguage::TypeScript).expect("parse");
        let names: Vec<&str> = parsed
            .declarations
            .iter()
            .map(|d| d.name.as_str())
            .collect();
        assert!(names.contains(&"Counter"));
        assert!(names.contains(&"increment"));
        assert!(names.contains(&"reset"));
        let kinds: Vec<DeclarationKind> =
            parsed.declarations.iter().map(|d| d.kind.clone()).collect();
        assert!(kinds.contains(&DeclarationKind::Class));
        assert!(kinds.contains(&DeclarationKind::Method));
    }

    #[test]
    fn typescript_extracts_imports() {
        let source = r#"
import { useState } from "react";
import path from "path";
import "./side-effect";
"#;
        let parsed = parse(source, SourceLanguage::TypeScript).expect("parse");
        let sources: Vec<&str> = parsed.imports.iter().map(|i| i.source.as_str()).collect();
        assert!(sources.contains(&"react"));
        assert!(sources.contains(&"path"));
        assert!(sources.contains(&"./side-effect"));
    }

    #[test]
    fn javascript_extracts_function_and_class() {
        let source = r#"
export function greet(name) {
    return "hi " + name;
}
class Animal {
    speak() {}
}
"#;
        let parsed = parse(source, SourceLanguage::JavaScript).expect("parse");
        let names: Vec<&str> = parsed
            .declarations
            .iter()
            .map(|d| d.name.as_str())
            .collect();
        assert!(names.contains(&"greet"));
        assert!(names.contains(&"Animal"));
        assert!(names.contains(&"speak"));
    }

    #[test]
    fn python_extracts_function() {
        let source = "def add(a, b):\n    return a + b\n";
        let parsed = parse(source, SourceLanguage::Python).expect("parse");
        assert_eq!(parsed.declarations.len(), 1);
        assert_eq!(parsed.declarations[0].name, "add");
        assert_eq!(parsed.declarations[0].kind, DeclarationKind::Function);
    }

    #[test]
    fn python_extracts_class_with_methods() {
        let source = r"
class Counter:
    def __init__(self):
        self.count = 0

    def increment(self):
        self.count += 1
";
        let parsed = parse(source, SourceLanguage::Python).expect("parse");
        let names: Vec<&str> = parsed
            .declarations
            .iter()
            .map(|d| d.name.as_str())
            .collect();
        assert!(names.contains(&"Counter"));
        assert!(names.contains(&"__init__"));
        assert!(names.contains(&"increment"));
    }

    #[test]
    fn python_extracts_imports_from_both_forms() {
        let source = r"
import os
import sys
from collections import defaultdict
from typing import Optional, List
";
        let parsed = parse(source, SourceLanguage::Python).expect("parse");
        let sources: Vec<&str> = parsed.imports.iter().map(|i| i.source.as_str()).collect();
        assert!(sources.contains(&"os"));
        assert!(sources.contains(&"sys"));
        assert!(sources.contains(&"collections"));
        assert!(sources.contains(&"typing"));
    }

    #[test]
    fn go_extracts_functions_and_methods() {
        let source = r"
package main

func Add(a int, b int) int {
    return a + b
}

func (c *Counter) Increment() {
    c.count++
}
";
        let parsed = parse(source, SourceLanguage::Go).expect("parse");
        let names: Vec<&str> = parsed
            .declarations
            .iter()
            .map(|d| d.name.as_str())
            .collect();
        assert!(names.contains(&"Add"));
        assert!(names.contains(&"Increment"));
        let kinds: Vec<DeclarationKind> =
            parsed.declarations.iter().map(|d| d.kind.clone()).collect();
        assert!(kinds.contains(&DeclarationKind::Function));
        assert!(kinds.contains(&DeclarationKind::Method));
    }

    #[test]
    fn go_extracts_struct_as_class() {
        let source = r"
package main

type Counter struct {
    count int
}
";
        let parsed = parse(source, SourceLanguage::Go).expect("parse");
        let counter = parsed
            .declarations
            .iter()
            .find(|d| d.name == "Counter")
            .expect("struct declaration");
        assert_eq!(counter.kind, DeclarationKind::Class);
        assert!(counter.end_byte > counter.start_byte);
    }

    #[test]
    fn go_extracts_imports_single_and_grouped() {
        let source = r#"
package main

import "fmt"

import (
    "os"
    "strings"
)
"#;
        let parsed = parse(source, SourceLanguage::Go).expect("parse");
        let sources: Vec<&str> = parsed.imports.iter().map(|i| i.source.as_str()).collect();
        assert!(sources.contains(&"fmt"));
        assert!(sources.contains(&"os"));
        assert!(sources.contains(&"strings"));
    }

    #[test]
    fn line_numbers_are_one_based() {
        let source = "\n\nfunction f() {}\n";
        let parsed = parse(source, SourceLanguage::JavaScript).expect("parse");
        assert_eq!(parsed.declarations[0].start_line, 3);
    }

    #[test]
    fn error_count_increments_on_invalid_source() {
        // Garbage-but-not-empty input — Tree-sitter inserts ERROR nodes.
        let source = "function (((((((( unbalanced";
        let parsed = parse(source, SourceLanguage::JavaScript).expect("parse");
        assert!(
            parsed.error_count > 0,
            "expected at least one ERROR node, got {:?}",
            parsed.error_count
        );
    }

    #[test]
    fn empty_source_parses_to_empty_file() {
        let parsed = parse("", SourceLanguage::TypeScript).expect("parse");
        assert_eq!(parsed.declarations.len(), 0);
        assert_eq!(parsed.imports.len(), 0);
        assert_eq!(parsed.exports.len(), 0);
    }

    #[test]
    fn declaration_kind_serializes_snake_case() {
        let json = serde_json::to_string(&DeclarationKind::Method).expect("serialize");
        assert_eq!(json, "\"method\"");
    }

    #[test]
    fn parsed_file_round_trips_through_json() {
        let source = "function f() {}\n";
        let parsed = parse(source, SourceLanguage::JavaScript).expect("parse");
        let json = serde_json::to_string(&parsed).expect("serialize");
        let back: ParsedFile = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.declarations.len(), parsed.declarations.len());
    }
}
