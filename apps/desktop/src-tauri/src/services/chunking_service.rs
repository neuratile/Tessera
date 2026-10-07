//! Semantic chunking — split source into RAG-indexable units at
//! AST-aware boundaries.
//!
//! Per ADR-0001 + rules.md §12.3: chunks land at function / class
//! boundaries (not arbitrary token splits) so retrieval surfaces
//! complete declarations rather than mid-function fragments. Target
//! size 500–1500 approximate tokens per chunk; smaller declarations
//! coalesce into a leading "module preamble" chunk, larger ones emit
//! as-is in Phase 3 (a Phase 5 enhancement may sub-split mega-classes).
//!
//! Token counting uses [`approximate_token_count`] from the LLM
//! provider layer — same heuristic across the producer (chunker) and
//! consumer (prompt assembler), so budget math stays consistent.

use serde::{Deserialize, Serialize};

use crate::providers::llm::approximate_token_count;
use crate::services::ast_service::{Declaration, DeclarationKind, ParsedFile};

/// Target lower bound — below this we coalesce trailing whitespace /
/// short helpers into the previous chunk. Picked to match `rules.md`
/// §12.3's "500–1500 token" guidance.
pub const TARGET_MIN_TOKENS: usize = 500;

/// Target upper bound — chunks above this size are still emitted but
/// flagged via `Chunk::oversize` so downstream consumers can route
/// them through summarization rather than direct injection. Phase 3
/// does not split them; that lands in Phase 5 if measured useful.
pub const TARGET_MAX_TOKENS: usize = 1500;

/// Logical chunk type — mirrors `DeclarationKind` plus the catch-all
/// `Module` for everything between named declarations (imports,
/// top-level statements, comment blocks).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChunkKind {
    Function,
    Method,
    Class,
    /// Top-of-file imports / constants / module-level statements that
    /// do not belong to any declaration.
    Module,
}

impl From<DeclarationKind> for ChunkKind {
    fn from(d: DeclarationKind) -> Self {
        match d {
            DeclarationKind::Function => Self::Function,
            DeclarationKind::Method => Self::Method,
            DeclarationKind::Class => Self::Class,
        }
    }
}

/// One chunk ready for embedding + storage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chunk {
    pub kind: ChunkKind,
    /// Identifier when the chunk maps to a named declaration; empty
    /// for module-level chunks.
    pub name: String,
    /// 1-based line range, inclusive on both ends.
    pub start_line: u32,
    pub end_line: u32,
    /// Chunk text — what gets embedded.
    pub content: String,
    /// Approximate token count from
    /// [`approximate_token_count`] (4 chars per token).
    pub token_count: usize,
    /// True when the chunk exceeds [`TARGET_MAX_TOKENS`]; consumers
    /// route oversize chunks through summarization rather than direct
    /// LLM injection.
    pub oversize: bool,
}

/// Split a parsed file into chunks. The original source string is
/// re-supplied (rather than embedded in `ParsedFile`) so memory does
/// not double up while many files sit in the analysis pipeline.
///
/// The algorithm:
/// 1. Walk declarations in source order.
/// 2. Emit a single `Module` chunk for everything before the first
///    declaration (imports, top-level constants, etc.). Skip if empty.
/// 3. Emit one chunk per declaration using its byte range.
/// 4. Methods inside classes are emitted *both* via the class chunk
///    (whole class) *and* as individual method chunks — the chunker is
///    permissive here because RAG benefits from fine-grained matches
///    even when redundant. Phase 5's `chunk_repo` deduplicates exact
///    text matches before embedding.
/// 5. Source after the last declaration emits as a trailing `Module`
///    chunk if it carries non-trivial content.
#[must_use]
pub fn chunk_source(source: &str, parsed: &ParsedFile) -> Vec<Chunk> {
    if source.is_empty() {
        return Vec::new();
    }

    let mut chunks = Vec::new();
    let bytes = source.as_bytes();
    let mut decls = parsed.declarations.clone();

    // Source-order required for the prefix / suffix Module chunks.
    decls.sort_by_key(|d| d.start_byte);

    // Prefix chunk: everything before the first top-level declaration.
    let first_top_byte = top_level_first_byte(&decls);
    if let Some(start) = first_top_byte {
        if start > 0 {
            push_module_slice(&mut chunks, source, bytes, 0, start);
        }
    } else if !source.trim().is_empty() {
        // No declarations at all — entire file is one Module chunk.
        push_module_slice(&mut chunks, source, bytes, 0, bytes.len());
        return chunks;
    }

    for decl in &decls {
        push_declaration_chunk(&mut chunks, source, bytes, decl);
    }

    // Suffix chunk: tail of the file after the last top-level
    // declaration (e.g. exports written after class definitions).
    if let Some(last_top_byte) = top_level_last_byte(&decls) {
        if last_top_byte < bytes.len() {
            push_module_slice(&mut chunks, source, bytes, last_top_byte, bytes.len());
        }
    }

    chunks
}

fn top_level_first_byte(decls: &[Declaration]) -> Option<usize> {
    decls
        .iter()
        .filter(|d| d.kind != DeclarationKind::Method)
        .map(|d| d.start_byte as usize)
        .min()
}

fn top_level_last_byte(decls: &[Declaration]) -> Option<usize> {
    decls
        .iter()
        .filter(|d| d.kind != DeclarationKind::Method)
        .map(|d| d.end_byte as usize)
        .max()
}

fn push_module_slice(
    chunks: &mut Vec<Chunk>,
    source: &str,
    bytes: &[u8],
    start: usize,
    end: usize,
) {
    let end = end.min(bytes.len());
    if start >= end {
        return;
    }
    let Some(slice) = source.get(start..end) else {
        return;
    };
    if slice.trim().is_empty() {
        return;
    }
    let (start_line, end_line) = byte_range_to_lines(source, start, end);
    let token_count = approximate_token_count(slice);
    chunks.push(Chunk {
        kind: ChunkKind::Module,
        name: String::new(),
        start_line,
        end_line,
        content: slice.to_string(),
        token_count,
        oversize: token_count > TARGET_MAX_TOKENS,
    });
}

fn push_declaration_chunk(chunks: &mut Vec<Chunk>, source: &str, bytes: &[u8], decl: &Declaration) {
    let start = (decl.start_byte as usize).min(bytes.len());
    let end = (decl.end_byte as usize).min(bytes.len());
    if start >= end {
        return;
    }
    let Some(slice) = source.get(start..end) else {
        return;
    };

    let token_count = approximate_token_count(slice);
    chunks.push(Chunk {
        kind: ChunkKind::from(decl.kind.clone()),
        name: decl.name.clone(),
        start_line: decl.start_line,
        end_line: decl.end_line,
        content: slice.to_string(),
        token_count,
        oversize: token_count > TARGET_MAX_TOKENS,
    });
}

/// Compute 1-based start / end line numbers for a byte range.
fn byte_range_to_lines(source: &str, start: usize, end: usize) -> (u32, u32) {
    let prefix_lines = source[..start].matches('\n').count();
    let span_lines = source[start..end.min(source.len())].matches('\n').count();
    let start_line = u32::try_from(prefix_lines)
        .unwrap_or(u32::MAX)
        .saturating_add(1);
    let end_line = start_line.saturating_add(u32::try_from(span_lines).unwrap_or(0));
    (start_line, end_line)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::ast_service;
    use crate::services::file_discovery_service::SourceLanguage;

    fn parse_ts(source: &str) -> ParsedFile {
        ast_service::parse(source, SourceLanguage::TypeScript).expect("parse")
    }

    fn parse_py(source: &str) -> ParsedFile {
        ast_service::parse(source, SourceLanguage::Python).expect("parse")
    }

    #[test]
    fn empty_source_yields_zero_chunks() {
        let parsed = ParsedFile::default();
        let chunks = chunk_source("", &parsed);
        assert_eq!(chunks.len(), 0);
    }

    #[test]
    fn config_file_with_no_declarations_emits_single_module_chunk() {
        // Simulate a config / docs file that the AST pipeline never
        // touched — `ParsedFile` carries no declarations.
        let parsed = ParsedFile::default();
        let source = "key = value\nother = 1\n";
        let chunks = chunk_source(source, &parsed);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].kind, ChunkKind::Module);
        assert_eq!(chunks[0].content, source);
    }

    #[test]
    fn function_declaration_yields_one_function_chunk() {
        let source = "function add(a: number, b: number): number {\n    return a + b;\n}\n";
        let parsed = parse_ts(source);
        let chunks = chunk_source(source, &parsed);
        let function_chunks: Vec<&Chunk> = chunks
            .iter()
            .filter(|c| c.kind == ChunkKind::Function)
            .collect();
        assert_eq!(function_chunks.len(), 1);
        assert_eq!(function_chunks[0].name, "add");
        assert!(function_chunks[0].content.contains("return a + b"));
    }

    #[test]
    fn imports_become_module_prefix_chunk() {
        let source = "import { useState } from \"react\";\n\nfunction f() {}\n";
        let parsed = parse_ts(source);
        let chunks = chunk_source(source, &parsed);
        // Expect a Module chunk (imports) plus a Function chunk.
        let module_chunks: Vec<&Chunk> = chunks
            .iter()
            .filter(|c| c.kind == ChunkKind::Module)
            .collect();
        assert_ne!(module_chunks.len(), 0);
        assert!(module_chunks[0].content.contains("import"));
    }

    #[test]
    fn class_with_methods_yields_class_and_method_chunks() {
        let source = "\
class Counter {
    increment(): void {
        this.count += 1;
    }
    reset(): void {
        this.count = 0;
    }
}
";
        let parsed = parse_ts(source);
        let chunks = chunk_source(source, &parsed);

        let class_chunks: Vec<&Chunk> = chunks
            .iter()
            .filter(|c| c.kind == ChunkKind::Class)
            .collect();
        assert_eq!(class_chunks.len(), 1);
        assert_eq!(class_chunks[0].name, "Counter");

        let method_chunks: Vec<&Chunk> = chunks
            .iter()
            .filter(|c| c.kind == ChunkKind::Method)
            .collect();
        assert_eq!(method_chunks.len(), 2);
        let method_names: Vec<&str> = method_chunks.iter().map(|c| c.name.as_str()).collect();
        assert!(method_names.contains(&"increment"));
        assert!(method_names.contains(&"reset"));
    }

    #[test]
    fn python_function_chunks_carry_correct_name_and_lines() {
        let source = "def foo(x):\n    return x + 1\n";
        let parsed = parse_py(source);
        let chunks = chunk_source(source, &parsed);
        let fn_chunks: Vec<&Chunk> = chunks
            .iter()
            .filter(|c| c.kind == ChunkKind::Function)
            .collect();
        assert_eq!(fn_chunks.len(), 1);
        assert_eq!(fn_chunks[0].name, "foo");
        assert_eq!(fn_chunks[0].start_line, 1);
        assert!(fn_chunks[0].end_line >= 2);
    }

    #[test]
    fn token_count_is_set_and_matches_heuristic() {
        let source = "function tiny() {}\n";
        let parsed = parse_ts(source);
        let chunks = chunk_source(source, &parsed);
        let fn_chunk = chunks
            .iter()
            .find(|c| c.kind == ChunkKind::Function)
            .expect("function chunk");
        // Heuristic = chars / 4 rounded up; the body is short so this
        // should be at most a handful of tokens.
        assert!(fn_chunk.token_count > 0);
        assert!(fn_chunk.token_count < 20);
        assert!(!fn_chunk.oversize);
    }

    #[test]
    fn oversize_flag_set_above_target_max() {
        // Build a fake declaration that spans a large body so we hit
        // the oversize gate without needing to engineer real source
        // beyond that size threshold.
        use std::fmt::Write as _;
        let mut body = String::with_capacity(8_000);
        body.push_str("function huge() {\n");
        for i in 0..1_500 {
            writeln!(body, "    const v{i} = {i};").expect("write to String");
        }
        body.push_str("}\n");
        let parsed = parse_ts(&body);
        let chunks = chunk_source(&body, &parsed);
        let fn_chunk = chunks
            .iter()
            .find(|c| c.kind == ChunkKind::Function)
            .expect("function chunk");
        assert!(
            fn_chunk.oversize,
            "expected oversize=true, got {fn_chunk:?}"
        );
        assert!(fn_chunk.token_count > TARGET_MAX_TOKENS);
    }

    #[test]
    fn module_suffix_after_last_declaration_emitted() {
        let source = "\
function first() {}

const SUFFIX = 1;
";
        let parsed = parse_ts(source);
        let chunks = chunk_source(source, &parsed);
        // Should have at least a Function chunk and a Module suffix
        // chunk for `const SUFFIX = 1;`.
        let kinds: Vec<ChunkKind> = chunks.iter().map(|c| c.kind).collect();
        assert!(kinds.contains(&ChunkKind::Function));
        assert!(kinds.contains(&ChunkKind::Module));
        // Suffix module chunk should mention SUFFIX.
        let mentions_suffix = chunks
            .iter()
            .any(|c| c.kind == ChunkKind::Module && c.content.contains("SUFFIX"));
        assert!(mentions_suffix);
    }

    #[test]
    fn chunks_are_ordered_by_source_position() {
        let source = "\
function first() {}
function second() {}
function third() {}
";
        let parsed = parse_ts(source);
        let chunks = chunk_source(source, &parsed);
        let fn_chunks: Vec<&Chunk> = chunks
            .iter()
            .filter(|c| c.kind == ChunkKind::Function)
            .collect();
        assert_eq!(fn_chunks.len(), 3);
        assert_eq!(fn_chunks[0].name, "first");
        assert_eq!(fn_chunks[1].name, "second");
        assert_eq!(fn_chunks[2].name, "third");
    }

    #[test]
    fn chunk_kind_serializes_snake_case() {
        assert_eq!(
            serde_json::to_string(&ChunkKind::Method).expect("serialize"),
            "\"method\""
        );
    }

    #[test]
    fn declaration_kind_converts_to_chunk_kind() {
        assert_eq!(
            ChunkKind::from(DeclarationKind::Function),
            ChunkKind::Function
        );
        assert_eq!(ChunkKind::from(DeclarationKind::Method), ChunkKind::Method);
        assert_eq!(ChunkKind::from(DeclarationKind::Class), ChunkKind::Class);
    }
}
