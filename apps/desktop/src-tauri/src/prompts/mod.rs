//! Versioned prompt templates.
//!
//! Per `rules.md` §12.1: each prompt is a typed function in its own file,
//! suffixed with a version (`_v1`, `_v2`). Never silently mutate a live
//! prompt — bump the version. All prompts produce structured output via
//! JSON Schema function-calling and are tested against the lowest-capability
//! target model (qwen2.5-coder:7b) first.
//!
//! Sub-modules added in Phase 4: `context_md_v1`, `test_plan_v1`,
//! `test_cases_v1`, `defect_report_v1`.
