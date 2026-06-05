-- Tessera Boards: initial schema
-- All tables use UUID primary keys and timestamptz columns.

-- ============================================================
-- USERS
-- ============================================================
CREATE TABLE users (
    id            UUID PRIMARY KEY,
    email         TEXT        NOT NULL UNIQUE,
    display_name  TEXT        NOT NULL,
    avatar_url    TEXT,
    -- Nullable: Supabase Auth keeps credentials in auth.users and the
    -- desktop client mirrors profiles here without a password hash. Only
    -- the self-hosted Axum auth path populates this column.
    password_hash TEXT,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_users_email ON users (email);

-- ============================================================
-- TEAMS
-- ============================================================
CREATE TABLE teams (
    id          UUID PRIMARY KEY,
    name        TEXT        NOT NULL,
    description TEXT,
    invite_code TEXT        NOT NULL UNIQUE,
    -- RESTRICT: deleting a user must not cascade-wipe every team they created
    -- (teams -> boards -> columns/sprints/issues -> comments/activity_logs).
    -- Ownership must be transferred before the account can be removed.
    created_by  UUID        NOT NULL REFERENCES users (id) ON DELETE RESTRICT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_teams_invite_code ON teams (invite_code);
CREATE INDEX idx_teams_created_by  ON teams (created_by);

-- ============================================================
-- TEAM MEMBERS
-- ============================================================
CREATE TABLE team_members (
    id        UUID PRIMARY KEY,
    team_id   UUID        NOT NULL REFERENCES teams (id) ON DELETE CASCADE,
    user_id   UUID        NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    role      TEXT        NOT NULL CHECK (role IN ('admin', 'member', 'viewer')),
    joined_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (team_id, user_id)
);

CREATE INDEX idx_team_members_team_id ON team_members (team_id);
CREATE INDEX idx_team_members_user_id ON team_members (user_id);

-- ============================================================
-- BOARDS
-- ============================================================
CREATE TABLE boards (
    id            UUID PRIMARY KEY,
    team_id       UUID        NOT NULL REFERENCES teams (id) ON DELETE CASCADE,
    name          TEXT        NOT NULL,
    key           TEXT        NOT NULL,
    description   TEXT,
    board_type    TEXT        NOT NULL DEFAULT 'kanban' CHECK (board_type IN ('kanban', 'scrum')),
    issue_counter INTEGER     NOT NULL DEFAULT 0,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (team_id, key)
);

CREATE INDEX idx_boards_team_id ON boards (team_id);

-- ============================================================
-- BOARD COLUMNS
-- ============================================================
CREATE TABLE board_columns (
    id        UUID PRIMARY KEY,
    board_id  UUID    NOT NULL REFERENCES boards (id) ON DELETE CASCADE,
    name      TEXT    NOT NULL,
    color     TEXT    NOT NULL DEFAULT '#6b7280',
    position  INTEGER NOT NULL,
    wip_limit INTEGER,
    -- Marks the column whose issues count as "completed" for sprint
    -- completion. Position is not a safe anchor (users can append columns
    -- after "Done"), so the flag is explicit.
    is_done   BOOLEAN NOT NULL DEFAULT FALSE,
    -- Deferred so multi-row reorders inside a transaction don't trip the
    -- constraint on intermediate states (checked at COMMIT instead).
    UNIQUE (board_id, position) DEFERRABLE INITIALLY DEFERRED
);

CREATE INDEX idx_board_columns_board_id ON board_columns (board_id);

-- ============================================================
-- SPRINTS
-- ============================================================
CREATE TABLE sprints (
    id         UUID PRIMARY KEY,
    board_id   UUID        NOT NULL REFERENCES boards (id) ON DELETE CASCADE,
    name       TEXT        NOT NULL,
    goal       TEXT,
    start_date TIMESTAMPTZ,
    end_date   TIMESTAMPTZ,
    status     TEXT        NOT NULL DEFAULT 'planned' CHECK (status IN ('planned', 'active', 'completed')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_sprints_board_id ON sprints (board_id);

-- ============================================================
-- ISSUES
-- ============================================================
CREATE TABLE issues (
    id           UUID PRIMARY KEY,
    board_id     UUID        NOT NULL REFERENCES boards (id) ON DELETE CASCADE,
    -- RESTRICT, not CASCADE: deleting a column must never destroy its issues.
    -- Both delete paths (Rust server + delete_column_atomic RPC) move issues
    -- to a fallback column before dropping the column row.
    column_id    UUID        NOT NULL REFERENCES board_columns (id) ON DELETE RESTRICT,
    sprint_id    UUID                 REFERENCES sprints (id) ON DELETE SET NULL,
    parent_id    UUID                 REFERENCES issues (id) ON DELETE CASCADE,
    issue_key    TEXT        NOT NULL,
    issue_type   TEXT        NOT NULL CHECK (issue_type IN ('epic', 'story', 'task', 'bug', 'subtask')),
    title        TEXT        NOT NULL,
    description  TEXT        NOT NULL DEFAULT '',
    priority     TEXT        NOT NULL DEFAULT 'medium' CHECK (priority IN ('critical', 'high', 'medium', 'low', 'trivial')),
    assignee_id  UUID                 REFERENCES users (id) ON DELETE SET NULL,
    reporter_id  UUID        NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    story_points INTEGER,
    due_date     TIMESTAMPTZ,
    git_branch   TEXT,
    position     INTEGER     NOT NULL DEFAULT 0,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (board_id, issue_key)
);

CREATE INDEX idx_issues_board_id    ON issues (board_id);
CREATE INDEX idx_issues_column_id   ON issues (column_id);
CREATE INDEX idx_issues_sprint_id   ON issues (sprint_id);
CREATE INDEX idx_issues_parent_id   ON issues (parent_id);
CREATE INDEX idx_issues_assignee_id ON issues (assignee_id);
CREATE INDEX idx_issues_reporter_id ON issues (reporter_id);

-- ============================================================
-- COMMENTS
-- ============================================================
CREATE TABLE comments (
    id         UUID PRIMARY KEY,
    issue_id   UUID        NOT NULL REFERENCES issues (id) ON DELETE CASCADE,
    author_id  UUID        NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    body       TEXT        NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_comments_issue_id  ON comments (issue_id);
CREATE INDEX idx_comments_author_id ON comments (author_id);

-- ============================================================
-- ACTIVITY LOGS
-- ============================================================
CREATE TABLE activity_logs (
    id         UUID PRIMARY KEY,
    issue_id   UUID        NOT NULL REFERENCES issues (id) ON DELETE CASCADE,
    user_id    UUID        NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    action     TEXT        NOT NULL,
    field      TEXT,
    old_value  TEXT,
    new_value  TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_activity_logs_issue_id ON activity_logs (issue_id);
CREATE INDEX idx_activity_logs_user_id  ON activity_logs (user_id);

-- ============================================================
-- LABELS
-- ============================================================
CREATE TABLE labels (
    id       UUID PRIMARY KEY,
    board_id UUID NOT NULL REFERENCES boards (id) ON DELETE CASCADE,
    name     TEXT NOT NULL,
    color    TEXT NOT NULL DEFAULT '#3b82f6',
    UNIQUE (board_id, name)
);

CREATE INDEX idx_labels_board_id ON labels (board_id);

-- ============================================================
-- ISSUE ↔ LABEL (junction)
-- ============================================================
CREATE TABLE issue_labels (
    issue_id UUID NOT NULL REFERENCES issues (id) ON DELETE CASCADE,
    label_id UUID NOT NULL REFERENCES labels (id) ON DELETE CASCADE,
    PRIMARY KEY (issue_id, label_id)
);

CREATE INDEX idx_issue_labels_label_id ON issue_labels (label_id);
