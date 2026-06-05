/* eslint-disable */
import type {
  Board,
  BoardColumn,
  BoardUser,
  Comment,
  CreateBoardInput,
  CreateCommentInput,
  CreateIssueInput,
  CreateLabelInput,
  CreateSprintInput,
  CreateTeamInput,
  Issue,
  Label,
  MoveIssueInput,
  Sprint,
  Team,
  TeamMember,
  TeamRole,
  UpdateIssueInput,
  ActivityLog,
} from '@testing-ide/shared';

import { supabase } from '@/lib/supabase';

// ─── Server URL management (NOP / Dummy for backward compatibility) ────

let serverUrl: string | null = null;

export function setServerUrl(url: string): void {
  serverUrl = url;
}

export function getServerUrl(): string | null {
  return serverUrl || 'Supabase Direct';
}

// ─── Converters for Database mappings ─────────────────────────────────

function mapUser(dbUser: any): BoardUser {
  return {
    id: dbUser.id,
    email: dbUser.email,
    displayName: dbUser.display_name,
    avatarUrl: dbUser.avatar_url || undefined,
  };
}

function mapTeam(dbTeam: any): Team {
  return {
    id: dbTeam.id,
    name: dbTeam.name,
    description: dbTeam.description || '',
    inviteCode: dbTeam.invite_code,
    createdBy: dbTeam.created_by,
    createdAt: dbTeam.created_at,
    updatedAt: dbTeam.updated_at,
  };
}

function mapTeamMember(dbMember: any): TeamMember {
  const member: TeamMember = {
    id: dbMember.id,
    teamId: dbMember.team_id,
    userId: dbMember.user_id,
    role: dbMember.role as TeamRole,
    joinedAt: dbMember.joined_at,
  };
  if (dbMember.users) {
    member.user = mapUser(dbMember.users);
  }
  return member;
}

function mapBoard(dbBoard: any): Board {
  return {
    id: dbBoard.id,
    teamId: dbBoard.team_id,
    name: dbBoard.name,
    key: dbBoard.key,
    description: dbBoard.description || '',
    boardType: dbBoard.board_type,
    issueCounter: dbBoard.issue_counter,
    createdAt: dbBoard.created_at,
    updatedAt: dbBoard.updated_at,
  };
}

function mapColumn(dbCol: any): BoardColumn {
  return {
    id: dbCol.id,
    boardId: dbCol.board_id,
    name: dbCol.name,
    color: dbCol.color,
    position: dbCol.position,
    wipLimit: dbCol.wip_limit || undefined,
    isDone: dbCol.is_done ?? false,
  };
}

/**
 * Resolve the caller's role on a team, throwing unless it is in
 * `allowed`. Mirrors the Rust server's role checks for the
 * Supabase-direct path (defense in depth on top of RLS).
 */
async function requireTeamRole(teamId: string, allowed: TeamRole[]): Promise<void> {
  const { data: { user } } = await supabase.auth.getUser();
  if (!user) throw new Error('Not authenticated');

  const { data, error } = await supabase
    .from('team_members')
    .select('role')
    .eq('team_id', teamId)
    .eq('user_id', user.id)
    .maybeSingle();

  if (error) throw new Error(error.message);
  if (!data || !allowed.includes(data.role as TeamRole)) {
    throw new Error('Not authorized for this operation');
  }
}

/** Resolve a board's team id. */
async function boardTeamId(boardId: string): Promise<string> {
  const { data, error } = await supabase
    .from('boards')
    .select('team_id')
    .eq('id', boardId)
    .single();
  if (error) throw new Error(error.message);
  return data.team_id;
}

function mapSprint(dbSprint: any): Sprint {
  return {
    id: dbSprint.id,
    boardId: dbSprint.board_id,
    name: dbSprint.name,
    goal: dbSprint.goal || '',
    startDate: dbSprint.start_date || '',
    endDate: dbSprint.end_date || '',
    status: dbSprint.status,
    createdAt: dbSprint.created_at,
  };
}

function mapLabel(dbLabel: any): Label {
  return {
    id: dbLabel.id,
    boardId: dbLabel.board_id,
    name: dbLabel.name,
    color: dbLabel.color,
  };
}

function mapIssue(dbIssue: any): Issue {
  const issue: Issue = {
    id: dbIssue.id,
    boardId: dbIssue.board_id,
    columnId: dbIssue.column_id,
    issueKey: dbIssue.issue_key,
    issueType: dbIssue.issue_type,
    title: dbIssue.title,
    description: dbIssue.description,
    priority: dbIssue.priority,
    reporterId: dbIssue.reporter_id,
    position: dbIssue.position,
    labels: dbIssue.issue_labels 
      ? dbIssue.issue_labels.map((il: any) => il.label ? mapLabel(il.label) : null).filter(Boolean) 
      : [],
    subtaskCount: dbIssue.subtask_count?.[0]?.count || 0,
    commentCount: dbIssue.comment_count?.[0]?.count || 0,
    createdAt: dbIssue.created_at,
    updatedAt: dbIssue.updated_at,
  };
  if (dbIssue.sprint_id) issue.sprintId = dbIssue.sprint_id;
  if (dbIssue.parent_id) issue.parentId = dbIssue.parent_id;
  if (dbIssue.assignee_id) issue.assigneeId = dbIssue.assignee_id;
  if (dbIssue.story_points !== null && dbIssue.story_points !== undefined) issue.storyPoints = dbIssue.story_points;
  if (dbIssue.due_date) issue.dueDate = dbIssue.due_date;
  if (dbIssue.git_branch) issue.gitBranch = dbIssue.git_branch;
  if (dbIssue.assignee) issue.assignee = mapUser(dbIssue.assignee);
  if (dbIssue.reporter) issue.reporter = mapUser(dbIssue.reporter);
  return issue;
}

function mapComment(dbComment: any): Comment {
  const comment: Comment = {
    id: dbComment.id,
    issueId: dbComment.issue_id,
    authorId: dbComment.author_id,
    body: dbComment.body,
    createdAt: dbComment.created_at,
    updatedAt: dbComment.updated_at,
  };
  if (dbComment.author) {
    comment.author = mapUser(dbComment.author);
  }
  return comment;
}

function mapActivityLog(dbLog: any): ActivityLog {
  const log: ActivityLog = {
    id: dbLog.id,
    issueId: dbLog.issue_id,
    userId: dbLog.user_id,
    action: dbLog.action,
    field: dbLog.field || '',
    oldValue: dbLog.old_value || '',
    newValue: dbLog.new_value || '',
    createdAt: dbLog.created_at,
  };
  if (dbLog.users) {
    log.user = mapUser(dbLog.users);
  }
  return log;
}

// ─── Auth ────────────────────────────────────────────────────────────

export async function serverRegister(
  email: string,
  password: string,
  name?: string,
): Promise<{ accessToken: string; refreshToken: string }> {
  const { data, error } = await supabase.auth.signUp({
    email,
    password,
    options: {
      data: {
        display_name: name || email.split('@')[0],
      },
    },
  });
  if (error) throw new Error(error.message);
  if (!data.session) {
    throw new Error('Registration successful! Please check your email or log in.');
  }

  // signUp only creates a row in auth.users — public.users must be populated
  // explicitly or every issue/comment JOIN against users will fail for this
  // account. Upsert keeps this idempotent across retries.
  if (data.user) {
    const { error: profileError } = await supabase.from('users').upsert({
      id: data.user.id,
      email,
      display_name: name || email.split('@')[0],
    });
    if (profileError) {
      throw new Error(`Failed to create user profile: ${profileError.message}`);
    }
  }

  return {
    accessToken: data.session.access_token,
    refreshToken: data.session.refresh_token,
  };
}

export async function serverLogin(
  email: string,
  password: string,
): Promise<{ accessToken: string; refreshToken: string }> {
  const { data, error } = await supabase.auth.signInWithPassword({
    email,
    password,
  });
  if (error) throw new Error(error.message);
  if (!data.session) throw new Error('Login failed: no session created');
  return {
    accessToken: data.session.access_token,
    refreshToken: data.session.refresh_token,
  };
}

export async function serverRefreshToken(
  refreshToken: string,
): Promise<{ accessToken: string; refreshToken: string }> {
  const { data, error } = await supabase.auth.refreshSession({
    refresh_token: refreshToken,
  });
  if (error) throw new Error(error.message);
  if (!data.session) throw new Error('Refresh session failed: no session');
  return {
    accessToken: data.session.access_token,
    refreshToken: data.session.refresh_token,
  };
}

export async function serverGetMe(): Promise<BoardUser> {
  const { data: { user }, error } = await supabase.auth.getUser();
  if (error || !user) throw new Error(error?.message || 'Not authenticated');

  const { data: profile } = await supabase
    .from('users')
    .select('*')
    .eq('id', user.id)
    .single();

  if (profile) {
    return mapUser(profile);
  }

  return {
    id: user.id,
    email: user.email || '',
    displayName: user.user_metadata?.display_name || user.user_metadata?.name || user.email?.split('@')[0] || 'User',
    avatarUrl: user.user_metadata?.avatar_url,
  };
}

// ─── Teams ───────────────────────────────────────────────────────────

export async function fetchTeams(): Promise<Team[]> {
  const { data: { user } } = await supabase.auth.getUser();
  if (!user) throw new Error('Not authenticated');

  const { data, error } = await supabase
    .from('team_members')
    .select('teams(*)')
    .eq('user_id', user.id);

  if (error) throw new Error(error.message);
  return (data || [])
    .map((item: any) => item.teams)
    .filter(Boolean)
    .map(mapTeam);
}

export async function createTeam(input: CreateTeamInput): Promise<Team> {
  const { data: { user } } = await supabase.auth.getUser();
  if (!user) throw new Error('Not authenticated');

  const teamId = crypto.randomUUID();
  // CSPRNG, 20 hex chars (~80 bits). Joining grants full team membership,
  // so the code space must withstand online brute force.
  const inviteCode = Array.from(crypto.getRandomValues(new Uint8Array(10)))
    .map((b) => b.toString(16).padStart(2, '0'))
    .join('')
    .toUpperCase();

  const { data: team, error: teamError } = await supabase
    .from('teams')
    .insert({
      id: teamId,
      name: input.name,
      description: input.description || '',
      invite_code: inviteCode,
      created_by: user.id,
    })
    .select()
    .single();

  if (teamError) throw new Error(teamError.message);

  // Add creator as team member
  const { error: memberError } = await supabase
    .from('team_members')
    .insert({
      id: crypto.randomUUID(),
      team_id: teamId,
      user_id: user.id,
      role: 'admin',
    });

  if (memberError) {
    await supabase.from('teams').delete().eq('id', teamId);
    throw new Error(memberError.message);
  }

  return mapTeam(team);
}

export async function joinTeam(inviteCode: string): Promise<TeamMember> {
  // SECURITY DEFINER RPC: under RLS a non-member cannot SELECT the team row
  // by invite code, so the lookup + insert happen server-side in one step.
  const { data: member, error } = await supabase.rpc('join_team_with_code', {
    p_invite_code: inviteCode.trim().toUpperCase(),
  });

  if (error) throw new Error(error.message);
  return mapTeamMember(member);
}

export async function fetchTeamMembers(teamId: string): Promise<TeamMember[]> {
  const { data, error } = await supabase
    .from('team_members')
    .select('*, users:user_id(*)')
    .eq('team_id', teamId);

  if (error) throw new Error(error.message);
  return (data || []).map(mapTeamMember);
}

/**
 * Throws when `memberId` is the last admin of the team — removing or
 * demoting them would leave the team permanently unadministrable.
 * Mirrors the Rust server's guard.
 */
async function assertNotLastAdmin(teamId: string, memberId: string): Promise<void> {
  const { data: target, error: targetError } = await supabase
    .from('team_members')
    .select('role')
    .eq('id', memberId)
    .eq('team_id', teamId)
    .single();
  if (targetError) throw new Error(targetError.message);
  if (target.role !== 'admin') return;

  const { count, error: countError } = await supabase
    .from('team_members')
    .select('id', { count: 'exact', head: true })
    .eq('team_id', teamId)
    .eq('role', 'admin');
  if (countError) throw new Error(countError.message);
  if ((count ?? 0) <= 1) {
    throw new Error('Cannot remove or demote the last admin of the team');
  }
}

export async function removeTeamMember(teamId: string, memberId: string): Promise<void> {
  await requireTeamRole(teamId, ['admin']);
  await assertNotLastAdmin(teamId, memberId);

  const { error } = await supabase
    .from('team_members')
    .delete()
    .eq('id', memberId)
    .eq('team_id', teamId);

  if (error) throw new Error(error.message);
}

export async function updateMemberRole(
  teamId: string,
  memberId: string,
  role: TeamRole,
): Promise<TeamMember> {
  await requireTeamRole(teamId, ['admin']);
  if (role !== 'admin') {
    await assertNotLastAdmin(teamId, memberId);
  }

  const { data, error } = await supabase
    .from('team_members')
    .update({ role })
    .eq('id', memberId)
    .eq('team_id', teamId)
    .select('*, users:user_id(*)')
    .single();

  if (error) throw new Error(error.message);
  return mapTeamMember(data);
}

// ─── Boards ──────────────────────────────────────────────────────────

export async function fetchBoards(teamId: string): Promise<Board[]> {
  const { data, error } = await supabase
    .from('boards')
    .select('*')
    .eq('team_id', teamId)
    .order('created_at', { ascending: true });

  if (error) throw new Error(error.message);
  return (data || []).map(mapBoard);
}

export async function createBoard(teamId: string, input: CreateBoardInput): Promise<Board> {
  const boardId = crypto.randomUUID();

  const { data: board, error: boardError } = await supabase
    .from('boards')
    .insert({
      id: boardId,
      team_id: teamId,
      name: input.name,
      key: input.key.trim().toUpperCase(),
      description: input.description || '',
      board_type: input.boardType || 'kanban',
      issue_counter: 0,
    })
    .select()
    .single();

  if (boardError) throw new Error(boardError.message);

  const defaultColumns = [
    { id: crypto.randomUUID(), board_id: boardId, name: 'To Do', color: '#6b7280', position: 0 },
    { id: crypto.randomUUID(), board_id: boardId, name: 'In Progress', color: '#3b82f6', position: 1 },
    { id: crypto.randomUUID(), board_id: boardId, name: 'In Review', color: '#f59e0b', position: 2 },
    { id: crypto.randomUUID(), board_id: boardId, name: 'Done', color: '#10b981', position: 3 },
  ];

  const { error: colsError } = await supabase
    .from('board_columns')
    .insert(defaultColumns);

  if (colsError) {
    await supabase.from('boards').delete().eq('id', boardId);
    throw new Error(colsError.message);
  }

  return mapBoard(board);
}

export async function fetchBoard(boardId: string): Promise<Board> {
  const { data, error } = await supabase
    .from('boards')
    .select('*')
    .eq('id', boardId)
    .single();

  if (error) throw new Error(error.message);
  return mapBoard(data);
}

export async function updateBoard(
  boardId: string,
  input: Partial<Pick<Board, 'name' | 'description'>>,
): Promise<Board> {
  const { data, error } = await supabase
    .from('boards')
    .update(input)
    .eq('id', boardId)
    .select()
    .single();

  if (error) throw new Error(error.message);
  return mapBoard(data);
}

export async function deleteBoard(boardId: string): Promise<void> {
  // Only team admins may delete boards — mirrors the Rust server.
  await requireTeamRole(await boardTeamId(boardId), ['admin']);

  const { error } = await supabase
    .from('boards')
    .delete()
    .eq('id', boardId);

  if (error) throw new Error(error.message);
}

// ─── Columns ─────────────────────────────────────────────────────────

export async function fetchColumns(boardId: string): Promise<BoardColumn[]> {
  const { data, error } = await supabase
    .from('board_columns')
    .select('*')
    .eq('board_id', boardId)
    .order('position', { ascending: true });

  if (error) throw new Error(error.message);
  return (data || []).map(mapColumn);
}

export async function createColumn(
  boardId: string,
  input: Pick<BoardColumn, 'name' | 'color'> & { wipLimit?: number },
): Promise<BoardColumn> {
  const { data: cols } = await supabase
    .from('board_columns')
    .select('position')
    .eq('board_id', boardId)
    .order('position', { ascending: false })
    .limit(1);

  const nextPos = (cols && cols.length > 0 && cols[0]?.position !== undefined) ? cols[0].position + 1 : 0;

  const { data, error } = await supabase
    .from('board_columns')
    .insert({
      id: crypto.randomUUID(),
      board_id: boardId,
      name: input.name,
      color: input.color,
      position: nextPos,
      wip_limit: input.wipLimit,
    })
    .select()
    .single();

  if (error) throw new Error(error.message);
  return mapColumn(data);
}

export async function updateColumn(
  columnId: string,
  input: Partial<Pick<BoardColumn, 'name' | 'color' | 'wipLimit'>>,
): Promise<BoardColumn> {
  const { data, error } = await supabase
    .from('board_columns')
    .update({
      name: input.name,
      color: input.color,
      wip_limit: input.wipLimit,
    })
    .eq('id', columnId)
    .select()
    .single();

  if (error) throw new Error(error.message);
  return mapColumn(data);
}

export async function deleteColumn(columnId: string): Promise<void> {
  // Viewers cannot modify board structure — mirrors the Rust server.
  const { data: column, error: columnError } = await supabase
    .from('board_columns')
    .select('board_id')
    .eq('id', columnId)
    .single();
  if (columnError) throw new Error(columnError.message);
  await requireTeamRole(await boardTeamId(column.board_id), ['admin', 'member']);

  const { error } = await supabase
    .from('board_columns')
    .delete()
    .eq('id', columnId);

  if (error) throw new Error(error.message);
}

export async function reorderColumns(
  boardId: string,
  columnIds: string[],
): Promise<BoardColumn[]> {
  // Two passes: parallel single-row updates would collide with the
  // UNIQUE (board_id, position) constraint. First park every column on a
  // scratch offset above the live range, then assign the final positions.
  const scratchUpdates = columnIds.map((id, index) =>
    supabase
      .from('board_columns')
      .update({ position: columnIds.length + index })
      .eq('id', id)
      .eq('board_id', boardId)
  );
  await Promise.all(scratchUpdates);

  const updates = columnIds.map((id, index) =>
    supabase
      .from('board_columns')
      .update({ position: index })
      .eq('id', id)
      .eq('board_id', boardId)
  );

  await Promise.all(updates);
  return fetchColumns(boardId);
}

// ─── Sprints ─────────────────────────────────────────────────────────

export async function fetchSprints(boardId: string): Promise<Sprint[]> {
  const { data, error } = await supabase
    .from('sprints')
    .select('*')
    .eq('board_id', boardId)
    .order('created_at', { ascending: true });

  if (error) throw new Error(error.message);
  return (data || []).map(mapSprint);
}

export async function createSprint(boardId: string, input: CreateSprintInput): Promise<Sprint> {
  const { data, error } = await supabase
    .from('sprints')
    .insert({
      id: crypto.randomUUID(),
      board_id: boardId,
      name: input.name,
      goal: input.goal || '',
      start_date: input.startDate || null,
      end_date: input.endDate || null,
      status: 'planned',
    })
    .select()
    .single();

  if (error) throw new Error(error.message);
  return mapSprint(data);
}

export async function updateSprint(
  sprintId: string,
  input: Partial<Pick<Sprint, 'name' | 'goal' | 'startDate' | 'endDate'>>,
): Promise<Sprint> {
  const { data, error } = await supabase
    .from('sprints')
    .update({
      name: input.name,
      goal: input.goal,
      start_date: input.startDate || null,
      end_date: input.endDate || null,
    })
    .eq('id', sprintId)
    .select()
    .single();

  if (error) throw new Error(error.message);
  return mapSprint(data);
}

export async function startSprint(sprintId: string): Promise<Sprint> {
  // Atomic RPC: the guard (one active sprint per board) and the status
  // update run in one transaction with the board row locked, so two
  // concurrent "Start Sprint" clicks cannot both succeed (TOCTOU).
  const { data, error } = await supabase.rpc('start_sprint_atomic', {
    p_sprint_id: sprintId,
  });

  if (error) throw new Error(error.message);
  return mapSprint(data);
}

export async function completeSprint(sprintId: string): Promise<Sprint> {
  // Mirror the server: incomplete issues (everything outside the rightmost
  // "Done" column) go back to the backlog instead of staying attached to a
  // completed sprint where no filter will ever show them.
  const { data: sprint, error: sprintError } = await supabase
    .from('sprints')
    .select('board_id')
    .eq('id', sprintId)
    .single();

  if (sprintError) throw new Error(sprintError.message);

  // The Done column carries an explicit is_done marker; highest position is
  // only a fallback for legacy boards (users can append columns after Done).
  const { data: doneColumn, error: doneError } = await supabase
    .from('board_columns')
    .select('id')
    .eq('board_id', sprint.board_id)
    .order('is_done', { ascending: false })
    .order('position', { ascending: false })
    .limit(1)
    .maybeSingle();

  if (doneError) throw new Error(doneError.message);

  let backlogQuery = supabase
    .from('issues')
    .update({ sprint_id: null })
    .eq('sprint_id', sprintId);
  if (doneColumn) {
    backlogQuery = backlogQuery.neq('column_id', doneColumn.id);
  }
  const { error: backlogError } = await backlogQuery;
  if (backlogError) throw new Error(backlogError.message);

  const { data, error } = await supabase
    .from('sprints')
    .update({ status: 'completed', end_date: new Date().toISOString() })
    .eq('id', sprintId)
    .select()
    .single();

  if (error) throw new Error(error.message);
  return mapSprint(data);
}

// ─── Issues ──────────────────────────────────────────────────────────

export async function fetchIssues(
  boardId: string,
  params?: { sprintId?: string; columnId?: string },
): Promise<Issue[]> {
  let query = supabase
    .from('issues')
    .select(`
      *,
      assignee:assignee_id(*),
      reporter:reporter_id(*),
      issue_labels(
        label:label_id(*)
      ),
      subtask_count:issues!parent_id(count),
      comment_count:comments(count)
    `)
    .eq('board_id', boardId);

  if (params?.sprintId) {
    if (params.sprintId === 'null' || params.sprintId === 'backlog') {
      query = query.is('sprint_id', null);
    } else {
      query = query.eq('sprint_id', params.sprintId);
    }
  }

  if (params?.columnId) {
    query = query.eq('column_id', params.columnId);
  }

  query = query.order('position', { ascending: true });

  const { data, error } = await query;
  if (error) throw new Error(error.message);
  return (data || []).map(mapIssue);
}

export async function createIssue(boardId: string, input: CreateIssueInput): Promise<Issue> {
  const { data: { user } } = await supabase.auth.getUser();
  if (!user) throw new Error('Not authenticated');

  const issueId = crypto.randomUUID();

  // Compute the next position in the target column — the column default of
  // 0 would leave every issue unordered and break drag-and-drop arithmetic.
  const { data: maxRow, error: maxError } = await supabase
    .from('issues')
    .select('position')
    .eq('column_id', input.columnId)
    .order('position', { ascending: false })
    .limit(1)
    .maybeSingle();
  if (maxError) throw new Error(maxError.message);
  const position = maxRow ? maxRow.position + 1 : 0;

  const { error } = await supabase
    .from('issues')
    .insert({
      id: issueId,
      board_id: boardId,
      column_id: input.columnId,
      position,
      sprint_id: input.sprintId || null,
      parent_id: input.parentId || null,
      issue_type: input.issueType,
      title: input.title,
      description: input.description || '',
      priority: input.priority,
      assignee_id: input.assigneeId || null,
      reporter_id: user.id,
      story_points: input.storyPoints || null,
      due_date: input.dueDate || null,
    })
    .select()
    .single();

  if (error) throw new Error(error.message);

  if (input.labelIds && input.labelIds.length > 0) {
    const labelInserts = input.labelIds.map(labelId => ({
      issue_id: issueId,
      label_id: labelId,
    }));
    const { error: labelsError } = await supabase
      .from('issue_labels')
      .insert(labelInserts);
    if (labelsError) throw new Error(labelsError.message);
  }

  return fetchIssue(issueId);
}

export async function fetchIssue(issueId: string): Promise<Issue> {
  const { data, error } = await supabase
    .from('issues')
    .select(`
      *,
      assignee:assignee_id(*),
      reporter:reporter_id(*),
      issue_labels(
        label:label_id(*)
      ),
      subtask_count:issues!parent_id(count),
      comment_count:comments(count)
    `)
    .eq('id', issueId)
    .single();

  if (error) throw new Error(error.message);
  return mapIssue(data);
}

export async function updateIssue(issueId: string, input: UpdateIssueInput): Promise<Issue> {
  const updatePayload: any = {};
  if (input.title !== undefined) updatePayload.title = input.title;
  if (input.description !== undefined) updatePayload.description = input.description;
  if (input.priority !== undefined) updatePayload.priority = input.priority;
  if (input.assigneeId !== undefined) updatePayload.assignee_id = input.assigneeId;
  if (input.sprintId !== undefined) updatePayload.sprint_id = input.sprintId;
  if (input.storyPoints !== undefined) updatePayload.story_points = input.storyPoints;
  if (input.dueDate !== undefined) updatePayload.due_date = input.dueDate;

  const { error } = await supabase
    .from('issues')
    .update(updatePayload)
    .eq('id', issueId);

  if (error) throw new Error(error.message);
  return fetchIssue(issueId);
}

export async function moveIssue(issueId: string, input: MoveIssueInput): Promise<Issue> {
  const { error } = await supabase.rpc('move_issue_on_board', {
    target_issue_id: issueId,
    new_column_id: input.columnId,
    new_position: input.position,
  });

  if (error) throw new Error(error.message);
  return fetchIssue(issueId);
}

export async function deleteIssue(issueId: string): Promise<void> {
  const { error } = await supabase
    .from('issues')
    .delete()
    .eq('id', issueId);

  if (error) throw new Error(error.message);
}

// ─── Comments ────────────────────────────────────────────────────────

export async function fetchComments(issueId: string): Promise<Comment[]> {
  const { data, error } = await supabase
    .from('comments')
    .select('*, author:author_id(*)')
    .eq('issue_id', issueId)
    .order('created_at', { ascending: true });

  if (error) throw new Error(error.message);
  return (data || []).map(mapComment);
}

export async function createComment(issueId: string, input: CreateCommentInput): Promise<Comment> {
  const { data: { user } } = await supabase.auth.getUser();
  if (!user) throw new Error('Not authenticated');

  const commentId = crypto.randomUUID();
  const { data, error } = await supabase
    .from('comments')
    .insert({
      id: commentId,
      issue_id: issueId,
      author_id: user.id,
      body: input.body,
    })
    .select('*, author:author_id(*)')
    .single();

  if (error) throw new Error(error.message);
  return mapComment(data);
}

/** Throws unless the current user authored the comment — mirrors the server. */
async function assertCommentAuthor(commentId: string): Promise<void> {
  const { data: { user } } = await supabase.auth.getUser();
  if (!user) throw new Error('Not authenticated');

  const { data, error } = await supabase
    .from('comments')
    .select('author_id')
    .eq('id', commentId)
    .single();
  if (error) throw new Error(error.message);
  if (data.author_id !== user.id) {
    throw new Error('Only the comment author can modify this comment');
  }
}

export async function updateComment(commentId: string, body: string): Promise<Comment> {
  await assertCommentAuthor(commentId);

  const { data, error } = await supabase
    .from('comments')
    .update({ body })
    .eq('id', commentId)
    .select('*, author:author_id(*)')
    .single();

  if (error) throw new Error(error.message);
  return mapComment(data);
}

export async function deleteComment(commentId: string): Promise<void> {
  await assertCommentAuthor(commentId);

  const { error } = await supabase
    .from('comments')
    .delete()
    .eq('id', commentId);

  if (error) throw new Error(error.message);
}

// ─── Labels ──────────────────────────────────────────────────────────

export async function fetchLabels(boardId: string): Promise<Label[]> {
  const { data, error } = await supabase
    .from('labels')
    .select('*')
    .eq('board_id', boardId)
    .order('name', { ascending: true });

  if (error) throw new Error(error.message);
  return (data || []).map(mapLabel);
}

export async function createLabel(boardId: string, input: CreateLabelInput): Promise<Label> {
  const { data, error } = await supabase
    .from('labels')
    .insert({
      id: crypto.randomUUID(),
      board_id: boardId,
      name: input.name,
      color: input.color,
    })
    .select()
    .single();

  if (error) throw new Error(error.message);
  return mapLabel(data);
}

export async function deleteLabel(labelId: string): Promise<void> {
  const { error } = await supabase
    .from('labels')
    .delete()
    .eq('id', labelId);

  if (error) throw new Error(error.message);
}

// ─── Activity Logs ───────────────────────────────────────────────────

export async function fetchActivityLogs(issueId: string): Promise<ActivityLog[]> {
  const { data, error } = await supabase
    .from('activity_logs')
    .select('*, users:user_id(*)')
    .eq('issue_id', issueId)
    .order('created_at', { ascending: false });

  if (error) throw new Error(error.message);
  return (data || []).map(mapActivityLog);
}
