-- Migration 0002 — Phase 4 compatibility contract.
--
-- Keeps the existing richer schema intact while adding compatibility
-- columns requested by the phase checklist / API contracts:
--   users.password_hash
--   projects.path
--   artifacts.type
--   artifacts.content
--   code_chunks.file_path
--
-- Existing deployments already have the base tables from 0001_init.sql.

ALTER TABLE users ADD COLUMN password_hash TEXT NOT NULL DEFAULT '';

ALTER TABLE projects ADD COLUMN path TEXT;
UPDATE projects
SET path = root_path
WHERE path IS NULL;

ALTER TABLE artifacts ADD COLUMN type TEXT;
UPDATE artifacts
SET type = artifact_type
WHERE type IS NULL;

ALTER TABLE artifacts ADD COLUMN content TEXT;
UPDATE artifacts
SET content = content_md
WHERE content IS NULL;

ALTER TABLE code_chunks ADD COLUMN file_path TEXT;
