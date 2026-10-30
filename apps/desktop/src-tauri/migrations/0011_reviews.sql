-- Additive review storage. Existing artifacts and runs remain authoritative.
CREATE TABLE review_sessions (
    id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    status TEXT NOT NULL CHECK(status IN ('queued','running','completed','failed','cancelled','interrupted')),
    sequence INTEGER NOT NULL CHECK(sequence >= 0),
    payload TEXT NOT NULL CHECK(json_valid(payload)),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX idx_review_sessions_project_created ON review_sessions(project_id, created_at);
CREATE INDEX idx_review_sessions_status ON review_sessions(status);
CREATE TRIGGER review_terminal_immutable BEFORE UPDATE ON review_sessions
WHEN OLD.status NOT IN ('queued','running')
BEGIN SELECT RAISE(ABORT, 'terminal review is immutable'); END;

CREATE TABLE review_snapshots (
    id TEXT PRIMARY KEY NOT NULL,
    review_id TEXT NOT NULL UNIQUE REFERENCES review_sessions(id) ON DELETE CASCADE,
    payload TEXT NOT NULL CHECK(json_valid(payload)),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TRIGGER review_snapshot_immutable BEFORE UPDATE ON review_snapshots
BEGIN SELECT RAISE(ABORT, 'snapshot is immutable'); END;
CREATE TABLE review_sources (
    id TEXT PRIMARY KEY NOT NULL,
    snapshot_id TEXT NOT NULL REFERENCES review_snapshots(id) ON DELETE CASCADE,
    side TEXT NOT NULL CHECK(side IN ('old','new')),
    path TEXT NOT NULL,
    content BLOB NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(snapshot_id, side, path)
);
CREATE TRIGGER review_source_immutable BEFORE UPDATE ON review_sources
BEGIN SELECT RAISE(ABORT, 'source is immutable'); END;

CREATE TABLE review_steps (
    id TEXT PRIMARY KEY NOT NULL,
    review_id TEXT NOT NULL REFERENCES review_sessions(id) ON DELETE CASCADE,
    payload TEXT NOT NULL CHECK(json_valid(payload)),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX idx_review_steps_review ON review_steps(review_id);
CREATE TABLE review_findings (
    id TEXT PRIMARY KEY NOT NULL,
    review_id TEXT NOT NULL REFERENCES review_sessions(id) ON DELETE CASCADE,
    payload TEXT NOT NULL CHECK(json_valid(payload)),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX idx_review_findings_review ON review_findings(review_id);
CREATE TABLE review_artifact_links (
    id TEXT PRIMARY KEY NOT NULL,
    finding_id TEXT NOT NULL REFERENCES review_findings(id) ON DELETE CASCADE,
    artifact_id TEXT NOT NULL REFERENCES artifacts(id) ON DELETE NO ACTION DEFERRABLE INITIALLY DEFERRED,
    payload TEXT NOT NULL CHECK(json_valid(payload)),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX idx_review_artifact_links_finding ON review_artifact_links(finding_id);
CREATE INDEX idx_review_artifact_links_artifact ON review_artifact_links(artifact_id);
CREATE TABLE review_run_links (
    id TEXT PRIMARY KEY NOT NULL,
    artifact_link_id TEXT NOT NULL REFERENCES review_artifact_links(id) ON DELETE CASCADE,
    run_id TEXT NOT NULL UNIQUE REFERENCES test_runs(id) ON DELETE NO ACTION DEFERRABLE INITIALLY DEFERRED,
    payload TEXT NOT NULL CHECK(json_valid(payload)),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX idx_review_run_links_artifact_link ON review_run_links(artifact_link_id);
CREATE TRIGGER review_attempt_immutable BEFORE UPDATE ON review_run_links
BEGIN SELECT RAISE(ABORT, 'evidence attempt is immutable'); END;
CREATE TRIGGER review_artifact_link_immutable BEFORE UPDATE ON review_artifact_links
BEGIN SELECT RAISE(ABORT, 'test binding is immutable'); END;
