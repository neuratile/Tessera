-- Migration 0002 — track model + prompt + token usage on every artifact.
--
-- Phase 5 generation_service records which model produced an artifact,
-- which prompt version was used, and how many tokens it cost. The data
-- powers regeneration UX (clone with same provider) and observability
-- (cost dashboards, model A/B comparisons, prompt-version churn).
--
-- Stored as JSON text rather than as separate columns: the shape will
-- evolve (cache hits, retries, latency percentiles) and keeping it
-- denormalized avoids a migration per field. The producer guarantees
-- the document is always valid JSON via serde_json serialization.

ALTER TABLE artifacts
ADD COLUMN generation_metadata TEXT NOT NULL DEFAULT '{}';
