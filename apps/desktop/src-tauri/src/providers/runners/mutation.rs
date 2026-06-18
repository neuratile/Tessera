//! Mutation testing — wire types + the pure mutant engine
//! (`plan/versions/v2/v2-feature-docs/MUTATION_TESTING.md`, Stage 1).
//!
//! Mutation testing answers the question line coverage cannot: *would the
//! suite fail if the code were wrong?* The engine takes a source file, applies
//! a single small edit (a "mutant") on a line the baseline suite actually
//! covered, and the orchestrator ([`crate::services::mutation_service`]) reruns
//! the unchanged suite against it. A suite that now fails **killed** the
//! mutant; a suite that still passes let it **survive** — a real gap.
//!
//! This module holds two things, both unit-test heavy and free of I/O:
//!
//! - The serde wire types ([`MutantStatus`], [`Mutant`], [`MutantResult`],
//!   [`MutationResult`]) mirrored by the Zod schemas in
//!   `packages/shared/src/schemas/mutation.schema.ts` — Rust serde is the
//!   source of truth (`rules.md` §12.3.1), Zod follows. Wire convention matches
//!   the rest of the IPC layer: structs serialize `camelCase`; the status enum
//!   serializes `snake_case`.
//! - The **pure** mutant engine ([`generate_mutants`], [`apply_mutant`],
//!   [`cap_mutants`]) — the new intellectual core. It walks the tree-sitter
//!   syntax tree directly (rather than [`crate::services::ast_service`]'s
//!   declaration-only [`ParsedFile`](crate::services::ast_service::ParsedFile),
//!   which does not expose operator nodes) so it can target binary operators,
//!   boolean literals, and returns. Walking *typed* nodes means an operator
//!   inside a string or comment is never mutated — those are `string` /
//!   `comment` nodes, not `binary_expression` operators.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use tree_sitter::{Node, Parser};

use super::RunnerLanguage;

/// Outcome of running the suite against one mutant (design §2). `snake_case`
/// wire form mirrors the sibling status enums and the Zod literals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MutantStatus {
    /// The suite failed against the mutant — the bug was caught. ✅
    Killed,
    /// The suite still passed against the mutant — a real gap. ❌
    Survived,
    /// The mutant did not compile / run (e.g. a type error from the edit);
    /// excluded from the score denominator (a mutant that won't build proves
    /// nothing about the suite, design §4).
    Errored,
}

impl MutantStatus {
    /// Stable string used in DB rows and IPC payloads. Matches the serde
    /// `snake_case` wire form.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Killed => "killed",
            Self::Survived => "survived",
            Self::Errored => "errored",
        }
    }

    /// Inverse of [`as_str`](Self::as_str). Returns `None` for any unrecognised
    /// string (corruption detection in the repository decode path).
    #[must_use]
    pub fn from_str_value(s: &str) -> Option<Self> {
        match s {
            "killed" => Some(Self::Killed),
            "survived" => Some(Self::Survived),
            "errored" => Some(Self::Errored),
            _ => None,
        }
    }
}

/// A single-edit mutation of one source file. Pure data — the engine emits one
/// per applicable operator site on a covered line; the orchestrator splices
/// `replacement` into `[byte_start, byte_end)` via [`apply_mutant`] and reruns
/// the suite. Mirrors `MutantSchema` (camelCase wire form).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mutant {
    /// Workspace-relative path of the source file this mutant edits.
    pub file: String,
    /// 1-based line the edit sits on (the line whose coverage gated it in).
    pub line: u32,
    /// Stable kind of edit: `arithmetic` / `relational` / `logical` /
    /// `boolean_literal` / `return_negation`.
    pub operator_id: String,
    /// The original token / expression text the edit replaces (e.g. `>`),
    /// shown in the survivor list as `original → replacement`.
    pub original: String,
    /// The text spliced in (e.g. `>=`).
    pub replacement: String,
    /// 0-based byte offset of the splice start.
    pub byte_start: u32,
    /// 0-based byte offset of the splice end (exclusive).
    pub byte_end: u32,
}

/// One mutant paired with the suite's verdict against it. Mirrors
/// `MutantResultSchema`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MutantResult {
    pub mutant: Mutant,
    pub status: MutantStatus,
}

/// Aggregate result of a mutation-score run, returned to the renderer and
/// persisted (design §5.1). Mirrors `MutationResultSchema`.
///
/// `score = killed / (killed + survived)` — errored mutants leave the
/// denominator (design §4). `total = killed + survived + errored`.
/// `dropped_count` is how many generated mutants were dropped by the cap
/// (design §4 — a bounded sweep must say so, never silently truncate).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MutationResult {
    /// `killed / (killed + survived)`, in `[0, 1]`; `0.0` when no mutant was
    /// scorable (`total == 0`) — the UI distinguishes that case via `total`.
    pub score: f64,
    pub killed: u32,
    pub survived: u32,
    pub errored: u32,
    /// `killed + survived + errored` — the number of mutants actually run.
    pub total: u32,
    /// The persisted baseline run this score was measured against (its
    /// coverage gated which lines were mutated).
    pub baseline_run_id: String,
    pub mutants: Vec<MutantResult>,
    /// Mutants generated but dropped by the cap (design §4).
    pub dropped_count: u32,
}

/// One entry in an artifact's persisted mutation-score history (design §5.5).
/// A lightweight header for the trend list — the per-mutant detail is fetched
/// on demand as a [`MutationCheckRecord`]. Mirrors `MutationCheckSummarySchema`.
/// `baseline_run_id` is omitted (serde `None`) only if that run row was later
/// purged (the FK is `ON DELETE SET NULL`). `created_at` is RFC-3339.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MutationCheckSummary {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub baseline_run_id: Option<String>,
    pub score: f64,
    pub killed: u32,
    pub survived: u32,
    pub errored: u32,
    pub total: u32,
    pub dropped_count: u32,
    pub created_at: String,
}

/// A persisted mutation check with its full per-mutant list (design §5.5). The
/// detail behind a [`MutationCheckSummary`], re-rendered with the same survivor
/// UI as a live check. Mirrors `MutationCheckRecordSchema`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MutationCheckRecord {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub baseline_run_id: Option<String>,
    pub score: f64,
    pub killed: u32,
    pub survived: u32,
    pub errored: u32,
    pub total: u32,
    pub dropped_count: u32,
    pub created_at: String,
    pub mutants: Vec<MutantResult>,
}

// ---------------------------------------------------------------------------
// Pure mutant engine.
// ---------------------------------------------------------------------------

/// Walk the tree-sitter syntax tree of `source` and emit one [`Mutant`] per
/// applicable operator site that lands on a **covered** line (design §5.2).
/// **Pure** — no I/O, deterministic for a given input.
///
/// v1 operators (JS/TS only):
/// 1. **Arithmetic** — `+ - * / %` swapped.
/// 2. **Relational / equality** — `> >= < <=` boundary-shifted, `== != === !==`
///    swapped.
/// 3. **Logical** — `&&` ↔ `||`.
/// 4. **Boolean literal** — `true` ↔ `false`.
/// 5. **Return negation** — a `return <expr>` becomes `return !(<expr>)`.
///
/// Mutants on uncovered lines are never emitted (they are guaranteed survivors,
/// so scoring them is pure noise — design §2). Python returns no mutants in v1:
/// the engine is per-grammar and the orchestrator is language-agnostic, so the
/// other languages slot in later (design §7) without touching the loop.
// `covered_lines` is always built with the default hasher by the orchestrator;
// genericizing over `BuildHasher` would only thread a useless type param through
// every private walker helper.
#[allow(clippy::implicit_hasher)]
#[must_use]
pub fn generate_mutants(
    file: &str,
    source: &str,
    language: RunnerLanguage,
    covered_lines: &HashSet<u32>,
) -> Vec<Mutant> {
    let grammar = match language {
        RunnerLanguage::JavaScript => tree_sitter_javascript::language(),
        RunnerLanguage::TypeScript => tree_sitter_typescript::language_typescript(),
        // v1 is JS/TS only (design §3); Python/Go absorb the same engine later.
        RunnerLanguage::Python => return Vec::new(),
    };

    let mut parser = Parser::new();
    if parser.set_language(&grammar).is_err() {
        // A grammar ABI mismatch is a build-time problem, not user input; a
        // mutation sweep degrades to "no mutants" rather than failing the run.
        return Vec::new();
    }
    let Some(tree) = parser.parse(source, None) else {
        return Vec::new();
    };

    let mut out = Vec::new();
    collect_mutants(tree.root_node(), file, source, covered_lines, &mut out);
    out
}

/// Recursively visit `node`, pushing a [`Mutant`] for every operator site on a
/// covered line.
fn collect_mutants(
    node: Node<'_>,
    file: &str,
    source: &str,
    covered_lines: &HashSet<u32>,
    out: &mut Vec<Mutant>,
) {
    match node.kind() {
        "binary_expression" => {
            if let Some(op) = node.child_by_field_name("operator") {
                push_operator_mutant(op, file, covered_lines, out);
            }
        }
        "true" | "false" => push_boolean_mutant(node, file, covered_lines, out),
        "return_statement" => push_return_mutant(node, file, source, covered_lines, out),
        _ => {}
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_mutants(child, file, source, covered_lines, out);
    }
}

/// Emit an arithmetic / relational / logical operator swap, if the operator is
/// one we mutate and its line was covered.
fn push_operator_mutant(
    op: Node<'_>,
    file: &str,
    covered_lines: &HashSet<u32>,
    out: &mut Vec<Mutant>,
) {
    let Some((replacement, operator_id)) = swap_operator(op.kind()) else {
        return;
    };
    let line = line_of(op);
    if !covered_lines.contains(&line) {
        return;
    }
    out.push(Mutant {
        file: file.to_string(),
        line,
        operator_id: operator_id.to_string(),
        original: op.kind().to_string(),
        replacement: replacement.to_string(),
        byte_start: byte_of(op.start_byte()),
        byte_end: byte_of(op.end_byte()),
    });
}

/// Emit a `true` ↔ `false` flip on a covered line.
fn push_boolean_mutant(
    node: Node<'_>,
    file: &str,
    covered_lines: &HashSet<u32>,
    out: &mut Vec<Mutant>,
) {
    let line = line_of(node);
    if !covered_lines.contains(&line) {
        return;
    }
    // The replacement is fixed by the node kind — no source text needed.
    let (original, replacement) = match node.kind() {
        "true" => ("true", "false"),
        _ => ("false", "true"),
    };
    out.push(Mutant {
        file: file.to_string(),
        line,
        operator_id: "boolean_literal".to_string(),
        original: original.to_string(),
        replacement: replacement.to_string(),
        byte_start: byte_of(node.start_byte()),
        byte_end: byte_of(node.end_byte()),
    });
}

/// Emit a return-negation: `return <expr>` → `return !(<expr>)`, on a covered
/// line. A bare `return;` (no expression) is left alone.
fn push_return_mutant(
    node: Node<'_>,
    file: &str,
    source: &str,
    covered_lines: &HashSet<u32>,
    out: &mut Vec<Mutant>,
) {
    let Some(expr) = node.named_child(0) else {
        return;
    };
    let line = line_of(expr);
    if !covered_lines.contains(&line) {
        return;
    }
    let Ok(original) = expr.utf8_text(source.as_bytes()) else {
        return;
    };
    out.push(Mutant {
        file: file.to_string(),
        line,
        operator_id: "return_negation".to_string(),
        original: original.to_string(),
        replacement: format!("!({original})"),
        byte_start: byte_of(expr.start_byte()),
        byte_end: byte_of(expr.end_byte()),
    });
}

/// Map an operator token to its mutation `(replacement, operator_id)`, or
/// `None` for operators we do not mutate (`instanceof`, `in`, `??`, bitwise…).
fn swap_operator(op: &str) -> Option<(&'static str, &'static str)> {
    let pair = match op {
        "+" => ("-", "arithmetic"),
        "-" => ("+", "arithmetic"),
        "*" => ("/", "arithmetic"),
        "/" | "%" => ("*", "arithmetic"),
        ">" => (">=", "relational"),
        ">=" => (">", "relational"),
        "<" => ("<=", "relational"),
        "<=" => ("<", "relational"),
        "==" => ("!=", "relational"),
        "!=" => ("==", "relational"),
        "===" => ("!==", "relational"),
        "!==" => ("===", "relational"),
        "&&" => ("||", "logical"),
        "||" => ("&&", "logical"),
        _ => return None,
    };
    Some(pair)
}

/// Apply one mutant to `source` by splicing `replacement` into its byte range.
/// **Pure**. Returns the source unchanged when the byte range is out of bounds
/// or not on a UTF-8 boundary (a defensive no-op — the engine only ever emits
/// in-bounds, boundary-aligned ranges from a real parse).
#[must_use]
pub fn apply_mutant(source: &str, mutant: &Mutant) -> String {
    let start = mutant.byte_start as usize;
    let end = mutant.byte_end as usize;
    if start > end || end > source.len() || !source.is_char_boundary(start) || !source.is_char_boundary(end) {
        return source.to_string();
    }
    let mut out = String::with_capacity(source.len() - (end - start) + mutant.replacement.len());
    out.push_str(&source[..start]);
    out.push_str(&mutant.replacement);
    out.push_str(&source[end..]);
    out
}

/// Default mutant cap and its clamp bounds (design §4). The UI value is only a
/// hint — the orchestrator re-clamps so a tampered IPC payload cannot force a
/// thousand-run sweep.
pub const MUT_DEFAULT_MAX_MUTANTS: u32 = 40;
pub const MUT_MIN_MUTANTS: u32 = 1;
pub const MUT_MAX_MUTANTS: u32 = 200;

/// Cap `mutants` to `max` by deterministic strided sampling, returning the kept
/// list and the number dropped (design §4 — never silently truncate). `max` is
/// re-clamped to `[MUT_MIN_MUTANTS, MUT_MAX_MUTANTS]`.
///
/// Sampling spreads the kept mutants evenly across the input (every
/// `total / max`-th) rather than taking a prefix, so a file's later operators
/// are represented too. Deterministic: the same input always yields the same
/// sample (no RNG — `rules.md` forbids `Math.random`-style nondeterminism in a
/// reproducible sweep).
#[must_use]
pub fn cap_mutants(mut mutants: Vec<Mutant>, max: u32) -> (Vec<Mutant>, u32) {
    let cap = max.clamp(MUT_MIN_MUTANTS, MUT_MAX_MUTANTS) as usize;
    let total = mutants.len();
    if total <= cap {
        return (mutants, 0);
    }
    // Stable, deterministic order before sampling (the tree walk is already
    // deterministic, but sort makes the sample independent of node-visit order).
    mutants.sort_by(|a, b| {
        a.file
            .cmp(&b.file)
            .then(a.byte_start.cmp(&b.byte_start))
            .then(a.operator_id.cmp(&b.operator_id))
    });
    let kept: Vec<Mutant> = (0..cap)
        .map(|i| mutants[i * total / cap].clone())
        .collect();
    let dropped = u32::try_from(total - kept.len()).unwrap_or(u32::MAX);
    (kept, dropped)
}

fn line_of(node: Node<'_>) -> u32 {
    u32::try_from(node.start_position().row)
        .unwrap_or(u32::MAX)
        .saturating_add(1)
}

fn byte_of(offset: usize) -> u32 {
    u32::try_from(offset).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every line covered — isolates operator detection from coverage gating.
    fn all_lines() -> HashSet<u32> {
        (1..=10_000).collect()
    }

    fn gen(source: &str) -> Vec<Mutant> {
        generate_mutants("src/x.ts", source, RunnerLanguage::TypeScript, &all_lines())
    }

    #[test]
    fn mutant_status_round_trips_through_serde() {
        for (variant, expected) in [
            (MutantStatus::Killed, "killed"),
            (MutantStatus::Survived, "survived"),
            (MutantStatus::Errored, "errored"),
        ] {
            assert_eq!(variant.as_str(), expected);
            assert_eq!(MutantStatus::from_str_value(expected), Some(variant));
            let json = serde_json::to_string(&variant).expect("serialize");
            assert_eq!(json, format!("\"{expected}\""));
        }
        assert_eq!(MutantStatus::from_str_value("escaped"), None);
    }

    #[test]
    fn arithmetic_operator_is_swapped() {
        let mutants = gen("export const f = (a, b) => a + b;");
        let arith: Vec<_> = mutants.iter().filter(|m| m.operator_id == "arithmetic").collect();
        assert_eq!(arith.len(), 1);
        assert_eq!(arith[0].original, "+");
        assert_eq!(arith[0].replacement, "-");
    }

    #[test]
    fn relational_operator_is_boundary_shifted() {
        let mutants = gen("export const f = (n) => n > 0;");
        let rel: Vec<_> = mutants.iter().filter(|m| m.operator_id == "relational").collect();
        assert_eq!(rel.len(), 1);
        assert_eq!(rel[0].original, ">");
        assert_eq!(rel[0].replacement, ">=");
    }

    #[test]
    fn logical_operator_is_swapped() {
        let mutants = gen("export const f = (a, b) => a && b;");
        let log: Vec<_> = mutants.iter().filter(|m| m.operator_id == "logical").collect();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].original, "&&");
        assert_eq!(log[0].replacement, "||");
    }

    #[test]
    fn boolean_literal_is_flipped() {
        let mutants = gen("export const f = () => true;");
        let boolean: Vec<_> = mutants.iter().filter(|m| m.operator_id == "boolean_literal").collect();
        assert_eq!(boolean.len(), 1);
        assert_eq!(boolean[0].original, "true");
        assert_eq!(boolean[0].replacement, "false");
    }

    #[test]
    fn return_is_negated() {
        let mutants = gen("export function f(n) {\n  return n > 0;\n}");
        let ret: Vec<_> = mutants.iter().filter(|m| m.operator_id == "return_negation").collect();
        assert_eq!(ret.len(), 1);
        assert_eq!(ret[0].original, "n > 0");
        assert_eq!(ret[0].replacement, "!(n > 0)");
    }

    #[test]
    fn applying_a_mutant_splices_the_replacement() {
        let source = "export const f = (a, b) => a + b;";
        let mutants = gen(source);
        let arith = mutants.iter().find(|m| m.operator_id == "arithmetic").expect("arithmetic mutant");
        let mutated = apply_mutant(source, arith);
        assert_eq!(mutated, "export const f = (a, b) => a - b;");
        // Original is untouched (pure).
        assert!(source.contains("a + b"));
    }

    #[test]
    fn applying_a_return_negation_compiles_to_negated_expr() {
        let source = "export function f(n) {\n  return n > 0;\n}";
        let mutants = gen(source);
        let ret = mutants.iter().find(|m| m.operator_id == "return_negation").expect("return mutant");
        let mutated = apply_mutant(source, ret);
        assert!(mutated.contains("return !(n > 0);"), "got: {mutated}");
    }

    #[test]
    fn out_of_bounds_mutant_is_a_no_op() {
        let source = "const x = 1;";
        let bogus = Mutant {
            file: "x.ts".into(),
            line: 1,
            operator_id: "arithmetic".into(),
            original: "+".into(),
            replacement: "-".into(),
            byte_start: 1000,
            byte_end: 2000,
        };
        assert_eq!(apply_mutant(source, &bogus), source);
    }

    #[test]
    fn uncovered_lines_yield_no_mutants() {
        // Operator is on line 1, but only line 99 is "covered".
        let covered: HashSet<u32> = [99].into_iter().collect();
        let mutants = generate_mutants(
            "src/x.ts",
            "export const f = (a, b) => a + b;",
            RunnerLanguage::TypeScript,
            &covered,
        );
        assert!(mutants.is_empty(), "no mutant should survive coverage gating");
    }

    #[test]
    fn operators_inside_strings_and_comments_are_not_mutated() {
        // The `+`, `>`, `true` here live in a string and a comment — they are
        // `string` / `comment` nodes, not operators, so the typed walk skips
        // them. Only the real `a + b` operator is mutated.
        let source = r#"
// a + b should not mutate and true is fine
export const f = (a, b) => {
  const note = "x > y and true || false";
  return a + b;
};
"#;
        let mutants = gen(source);
        // Exactly one arithmetic (the real `a + b`), one return negation.
        assert_eq!(mutants.iter().filter(|m| m.operator_id == "arithmetic").count(), 1);
        assert_eq!(mutants.iter().filter(|m| m.operator_id == "relational").count(), 0);
        assert_eq!(mutants.iter().filter(|m| m.operator_id == "boolean_literal").count(), 0);
        assert_eq!(mutants.iter().filter(|m| m.operator_id == "logical").count(), 0);
    }

    #[test]
    fn empty_and_python_sources_yield_no_mutants() {
        assert!(gen("").is_empty());
        let py = generate_mutants(
            "x.py",
            "def f(a, b):\n    return a + b\n",
            RunnerLanguage::Python,
            &all_lines(),
        );
        assert!(py.is_empty(), "Python is out of scope for v1");
    }

    #[test]
    fn cap_keeps_all_when_under_limit() {
        let mutants = gen("export const f = (a, b, c) => a + b + c;");
        let n = mutants.len();
        let (kept, dropped) = cap_mutants(mutants, 40);
        assert_eq!(kept.len(), n);
        assert_eq!(dropped, 0);
    }

    #[test]
    fn cap_samples_deterministically_and_reports_dropped() {
        // Build 10 distinct mutants via 10 arithmetic ops across lines.
        let source = (0..10)
            .map(|i| format!("export const f{i} = (a, b) => a + b;"))
            .collect::<Vec<_>>()
            .join("\n");
        let mutants = gen(&source);
        assert!(mutants.len() >= 10);
        let total = mutants.len();

        let (kept, dropped) = cap_mutants(mutants.clone(), 3);
        assert_eq!(kept.len(), 3);
        assert_eq!(dropped as usize, total - 3);

        // Deterministic: the same input + cap yields the identical sample.
        let (kept2, _) = cap_mutants(mutants, 3);
        assert_eq!(kept, kept2);
    }

    #[test]
    fn mutation_result_serializes_camel_case() {
        let result = MutationResult {
            score: 0.75,
            killed: 3,
            survived: 1,
            errored: 1,
            total: 5,
            baseline_run_id: "r1".into(),
            mutants: vec![MutantResult {
                mutant: Mutant {
                    file: "cart.ts".into(),
                    line: 42,
                    operator_id: "relational".into(),
                    original: ">".into(),
                    replacement: ">=".into(),
                    byte_start: 10,
                    byte_end: 11,
                },
                status: MutantStatus::Survived,
            }],
            dropped_count: 0,
        };
        let value = serde_json::to_value(&result).expect("serialize");
        assert_eq!(value["baselineRunId"], "r1");
        assert_eq!(value["mutants"][0]["mutant"]["operatorId"], "relational");
        assert_eq!(value["mutants"][0]["status"], "survived");
        assert_eq!(value["droppedCount"], 0);
        let back: MutationResult = serde_json::from_value(value).expect("round trip");
        assert_eq!(back.killed, 3);
    }
}
