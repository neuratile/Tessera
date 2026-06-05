-- Tessera Boards: Row-Level Security + atomic helper functions.
--
-- The desktop client talks to Supabase (PostgREST) directly with the user's
-- JWT, so without RLS every authenticated user can read and write every
-- team's data. All policies below scope access by team membership.
--
-- The Rust server connects as the table owner and is therefore unaffected
-- by these policies (owners bypass RLS unless FORCE is set).

-- ============================================================
-- HELPER FUNCTIONS (SECURITY DEFINER avoids RLS recursion on
-- team_members and keeps policy expressions cheap)
-- ============================================================

CREATE OR REPLACE FUNCTION public.member_role(p_team_id UUID)
RETURNS TEXT
LANGUAGE sql STABLE SECURITY DEFINER
SET search_path = public
AS $$
    SELECT role FROM team_members
    WHERE team_id = p_team_id AND user_id = auth.uid();
$$;

CREATE OR REPLACE FUNCTION public.is_team_member(p_team_id UUID)
RETURNS BOOLEAN
LANGUAGE sql STABLE SECURITY DEFINER
SET search_path = public
AS $$
    SELECT public.member_role(p_team_id) IS NOT NULL;
$$;

CREATE OR REPLACE FUNCTION public.is_team_admin(p_team_id UUID)
RETURNS BOOLEAN
LANGUAGE sql STABLE SECURITY DEFINER
SET search_path = public
AS $$
    SELECT public.member_role(p_team_id) = 'admin';
$$;

-- Writer roles (everything except viewer).
CREATE OR REPLACE FUNCTION public.is_team_writer(p_team_id UUID)
RETURNS BOOLEAN
LANGUAGE sql STABLE SECURITY DEFINER
SET search_path = public
AS $$
    SELECT public.member_role(p_team_id) IN ('admin', 'member');
$$;

CREATE OR REPLACE FUNCTION public.board_team_id(p_board_id UUID)
RETURNS UUID
LANGUAGE sql STABLE SECURITY DEFINER
SET search_path = public
AS $$
    SELECT team_id FROM boards WHERE id = p_board_id;
$$;

CREATE OR REPLACE FUNCTION public.issue_team_id(p_issue_id UUID)
RETURNS UUID
LANGUAGE sql STABLE SECURITY DEFINER
SET search_path = public
AS $$
    SELECT b.team_id FROM issues i JOIN boards b ON b.id = i.board_id
    WHERE i.id = p_issue_id;
$$;

-- ============================================================
-- ENABLE RLS
-- ============================================================

ALTER TABLE users         ENABLE ROW LEVEL SECURITY;
ALTER TABLE teams         ENABLE ROW LEVEL SECURITY;
ALTER TABLE team_members  ENABLE ROW LEVEL SECURITY;
ALTER TABLE boards        ENABLE ROW LEVEL SECURITY;
ALTER TABLE board_columns ENABLE ROW LEVEL SECURITY;
ALTER TABLE sprints       ENABLE ROW LEVEL SECURITY;
ALTER TABLE issues        ENABLE ROW LEVEL SECURITY;
ALTER TABLE comments      ENABLE ROW LEVEL SECURITY;
ALTER TABLE activity_logs ENABLE ROW LEVEL SECURITY;
ALTER TABLE labels        ENABLE ROW LEVEL SECURITY;
ALTER TABLE issue_labels  ENABLE ROW LEVEL SECURITY;

-- ============================================================
-- USERS — profiles are readable by any signed-in user (needed for
-- assignee/reporter/member joins); users manage only their own row.
--
-- Column-level grants hide password_hash: RLS row policies cannot
-- restrict columns, so a USING(true) SELECT policy alone would let any
-- authenticated client read every user's hash via PostgREST
-- (e.g. GET /users?select=email,password_hash).
-- ============================================================

REVOKE ALL ON users FROM anon, authenticated;
GRANT SELECT (id, email, display_name, avatar_url, created_at, updated_at)
    ON users TO authenticated;
GRANT INSERT (id, email, display_name, avatar_url)
    ON users TO authenticated;
GRANT UPDATE (email, display_name, avatar_url, updated_at)
    ON users TO authenticated;

CREATE POLICY users_select ON users
    FOR SELECT TO authenticated USING (true);
CREATE POLICY users_insert ON users
    FOR INSERT TO authenticated WITH CHECK (id = auth.uid());
CREATE POLICY users_update ON users
    FOR UPDATE TO authenticated USING (id = auth.uid());

-- ============================================================
-- TEAMS
-- ============================================================

CREATE POLICY teams_select ON teams
    FOR SELECT TO authenticated USING (public.is_team_member(id));
CREATE POLICY teams_insert ON teams
    FOR INSERT TO authenticated WITH CHECK (created_by = auth.uid());
CREATE POLICY teams_update ON teams
    FOR UPDATE TO authenticated USING (public.is_team_admin(id));
CREATE POLICY teams_delete ON teams
    FOR DELETE TO authenticated USING (public.is_team_admin(id));

-- ============================================================
-- TEAM MEMBERS
-- Creator bootstraps their own admin row; everyone else joins through
-- join_team_with_code() (SECURITY DEFINER, below).
-- ============================================================

CREATE POLICY team_members_select ON team_members
    FOR SELECT TO authenticated USING (public.is_team_member(team_id));
CREATE POLICY team_members_insert ON team_members
    FOR INSERT TO authenticated WITH CHECK (
        user_id = auth.uid()
        AND EXISTS (
            SELECT 1 FROM teams t
            WHERE t.id = team_id AND t.created_by = auth.uid()
        )
    );
CREATE POLICY team_members_update ON team_members
    FOR UPDATE TO authenticated USING (public.is_team_admin(team_id));
CREATE POLICY team_members_delete ON team_members
    FOR DELETE TO authenticated USING (
        user_id = auth.uid() OR public.is_team_admin(team_id)
    );

-- ============================================================
-- BOARDS
-- ============================================================

CREATE POLICY boards_select ON boards
    FOR SELECT TO authenticated USING (public.is_team_member(team_id));
CREATE POLICY boards_insert ON boards
    FOR INSERT TO authenticated WITH CHECK (public.is_team_writer(team_id));
CREATE POLICY boards_update ON boards
    FOR UPDATE TO authenticated USING (public.is_team_admin(team_id));
CREATE POLICY boards_delete ON boards
    FOR DELETE TO authenticated USING (public.is_team_admin(team_id));

-- ============================================================
-- BOARD COLUMNS / SPRINTS / LABELS — scoped through the parent board
-- ============================================================

CREATE POLICY board_columns_select ON board_columns
    FOR SELECT TO authenticated USING (public.is_team_member(public.board_team_id(board_id)));
CREATE POLICY board_columns_write ON board_columns
    FOR INSERT TO authenticated WITH CHECK (public.is_team_writer(public.board_team_id(board_id)));
CREATE POLICY board_columns_update ON board_columns
    FOR UPDATE TO authenticated USING (public.is_team_writer(public.board_team_id(board_id)));
CREATE POLICY board_columns_delete ON board_columns
    FOR DELETE TO authenticated USING (public.is_team_writer(public.board_team_id(board_id)));

CREATE POLICY sprints_select ON sprints
    FOR SELECT TO authenticated USING (public.is_team_member(public.board_team_id(board_id)));
CREATE POLICY sprints_insert ON sprints
    FOR INSERT TO authenticated WITH CHECK (public.is_team_writer(public.board_team_id(board_id)));
CREATE POLICY sprints_update ON sprints
    FOR UPDATE TO authenticated USING (public.is_team_writer(public.board_team_id(board_id)));
CREATE POLICY sprints_delete ON sprints
    FOR DELETE TO authenticated USING (public.is_team_writer(public.board_team_id(board_id)));

CREATE POLICY labels_select ON labels
    FOR SELECT TO authenticated USING (public.is_team_member(public.board_team_id(board_id)));
CREATE POLICY labels_insert ON labels
    FOR INSERT TO authenticated WITH CHECK (public.is_team_writer(public.board_team_id(board_id)));
CREATE POLICY labels_update ON labels
    FOR UPDATE TO authenticated USING (public.is_team_writer(public.board_team_id(board_id)));
CREATE POLICY labels_delete ON labels
    FOR DELETE TO authenticated USING (public.is_team_writer(public.board_team_id(board_id)));

-- ============================================================
-- ISSUES
-- ============================================================

CREATE POLICY issues_select ON issues
    FOR SELECT TO authenticated USING (public.is_team_member(public.board_team_id(board_id)));
CREATE POLICY issues_insert ON issues
    FOR INSERT TO authenticated WITH CHECK (public.is_team_writer(public.board_team_id(board_id)));
CREATE POLICY issues_update ON issues
    FOR UPDATE TO authenticated USING (public.is_team_writer(public.board_team_id(board_id)));
CREATE POLICY issues_delete ON issues
    FOR DELETE TO authenticated USING (public.is_team_writer(public.board_team_id(board_id)));

-- ============================================================
-- COMMENTS — members read, authors edit/delete their own
-- ============================================================

CREATE POLICY comments_select ON comments
    FOR SELECT TO authenticated USING (public.is_team_member(public.issue_team_id(issue_id)));
CREATE POLICY comments_insert ON comments
    FOR INSERT TO authenticated WITH CHECK (
        author_id = auth.uid()
        AND public.is_team_writer(public.issue_team_id(issue_id))
    );
CREATE POLICY comments_update ON comments
    FOR UPDATE TO authenticated USING (author_id = auth.uid());
CREATE POLICY comments_delete ON comments
    FOR DELETE TO authenticated USING (author_id = auth.uid());

-- ============================================================
-- ACTIVITY LOGS — members read; writes happen with the acting user's id
-- ============================================================

CREATE POLICY activity_logs_select ON activity_logs
    FOR SELECT TO authenticated USING (public.is_team_member(public.issue_team_id(issue_id)));
CREATE POLICY activity_logs_insert ON activity_logs
    FOR INSERT TO authenticated WITH CHECK (
        user_id = auth.uid()
        AND public.is_team_writer(public.issue_team_id(issue_id))
    );

-- ============================================================
-- ISSUE LABELS
-- ============================================================

CREATE POLICY issue_labels_select ON issue_labels
    FOR SELECT TO authenticated USING (public.is_team_member(public.issue_team_id(issue_id)));
CREATE POLICY issue_labels_insert ON issue_labels
    FOR INSERT TO authenticated WITH CHECK (public.is_team_writer(public.issue_team_id(issue_id)));
CREATE POLICY issue_labels_delete ON issue_labels
    FOR DELETE TO authenticated USING (public.is_team_writer(public.issue_team_id(issue_id)));

-- ============================================================
-- ATOMIC HELPERS (RPC) — operations the client cannot do safely with
-- row-at-a-time PostgREST calls.
-- ============================================================

-- Join a team by invite code. SECURITY DEFINER because the caller is not
-- yet a member and therefore cannot SELECT the team row under RLS.
CREATE OR REPLACE FUNCTION public.join_team_with_code(p_invite_code TEXT)
RETURNS team_members
LANGUAGE plpgsql SECURITY DEFINER
SET search_path = public
AS $$
DECLARE
    v_team_id UUID;
    v_member team_members;
BEGIN
    IF auth.uid() IS NULL THEN
        RAISE EXCEPTION 'not authenticated';
    END IF;

    SELECT id INTO v_team_id FROM teams
    WHERE invite_code = upper(trim(p_invite_code));

    IF v_team_id IS NULL THEN
        RAISE EXCEPTION 'invalid invite code';
    END IF;

    IF EXISTS (SELECT 1 FROM team_members WHERE team_id = v_team_id AND user_id = auth.uid()) THEN
        RAISE EXCEPTION 'already a member of this team';
    END IF;

    INSERT INTO team_members (id, team_id, user_id, role)
    VALUES (gen_random_uuid(), v_team_id, auth.uid(), 'member')
    RETURNING * INTO v_member;

    RETURN v_member;
END;
$$;

-- Move an issue across/within columns with consistent position shifting.
-- Mirrors the Rust server's move_issue handler; validates the target
-- column belongs to the issue's own board.
CREATE OR REPLACE FUNCTION public.move_issue_on_board(
    target_issue_id UUID,
    new_column_id UUID,
    new_position INTEGER
)
RETURNS VOID
LANGUAGE plpgsql SECURITY DEFINER
SET search_path = public
AS $$
DECLARE
    v_board_id UUID;
    v_old_column_id UUID;
    v_old_position INTEGER;
BEGIN
    SELECT board_id, column_id, position
    INTO v_board_id, v_old_column_id, v_old_position
    FROM issues WHERE id = target_issue_id
    FOR UPDATE;

    IF v_board_id IS NULL THEN
        RAISE EXCEPTION 'issue not found';
    END IF;

    IF NOT public.is_team_writer(public.board_team_id(v_board_id)) THEN
        RAISE EXCEPTION 'not authorized to move issues on this board';
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM board_columns
        WHERE id = new_column_id AND board_id = v_board_id
    ) THEN
        RAISE EXCEPTION 'column does not belong to this board';
    END IF;

    IF v_old_column_id = new_column_id THEN
        IF v_old_position < new_position THEN
            UPDATE issues SET position = position - 1
            WHERE column_id = v_old_column_id
              AND position > v_old_position AND position <= new_position;
        ELSIF v_old_position > new_position THEN
            UPDATE issues SET position = position + 1
            WHERE column_id = v_old_column_id
              AND position >= new_position AND position < v_old_position;
        END IF;
    ELSE
        UPDATE issues SET position = position - 1
        WHERE column_id = v_old_column_id AND position > v_old_position;

        UPDATE issues SET position = position + 1
        WHERE column_id = new_column_id AND position >= new_position;
    END IF;

    UPDATE issues
    SET column_id = new_column_id, position = new_position, updated_at = now()
    WHERE id = target_issue_id;
END;
$$;

-- Start a sprint atomically: lock the board row so two concurrent starts
-- cannot both pass the one-active-sprint-per-board guard.
CREATE OR REPLACE FUNCTION public.start_sprint_atomic(p_sprint_id UUID)
RETURNS sprints
LANGUAGE plpgsql SECURITY DEFINER
SET search_path = public
AS $$
DECLARE
    v_board_id UUID;
    v_status TEXT;
    v_sprint sprints;
BEGIN
    SELECT board_id, status INTO v_board_id, v_status
    FROM sprints WHERE id = p_sprint_id;

    IF v_board_id IS NULL THEN
        RAISE EXCEPTION 'sprint not found';
    END IF;

    IF NOT public.is_team_writer(public.board_team_id(v_board_id)) THEN
        RAISE EXCEPTION 'not authorized to start sprints on this board';
    END IF;

    IF v_status <> 'planned' THEN
        RAISE EXCEPTION 'only planned sprints can be started';
    END IF;

    -- Serialize concurrent starts on the same board.
    PERFORM 1 FROM boards WHERE id = v_board_id FOR UPDATE;

    IF EXISTS (SELECT 1 FROM sprints WHERE board_id = v_board_id AND status = 'active') THEN
        RAISE EXCEPTION 'another sprint is already active on this board';
    END IF;

    UPDATE sprints
    SET status = 'active', start_date = COALESCE(start_date, now())
    WHERE id = p_sprint_id AND status = 'planned'
    RETURNING * INTO v_sprint;

    RETURN v_sprint;
END;
$$;

-- Lock function execution down to signed-in users.
REVOKE ALL ON FUNCTION public.join_team_with_code(TEXT) FROM anon, public;
REVOKE ALL ON FUNCTION public.move_issue_on_board(UUID, UUID, INTEGER) FROM anon, public;
REVOKE ALL ON FUNCTION public.start_sprint_atomic(UUID) FROM anon, public;
GRANT EXECUTE ON FUNCTION public.join_team_with_code(TEXT) TO authenticated;
GRANT EXECUTE ON FUNCTION public.move_issue_on_board(UUID, UUID, INTEGER) TO authenticated;
GRANT EXECUTE ON FUNCTION public.start_sprint_atomic(UUID) TO authenticated;
