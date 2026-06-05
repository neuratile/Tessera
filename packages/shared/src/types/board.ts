// ─── Enums as union types ────────────────────────────────────────────

export type IssueType = 'epic' | 'story' | 'task' | 'bug' | 'subtask';
export type Priority = 'critical' | 'high' | 'medium' | 'low' | 'trivial';
export type BoardType = 'kanban' | 'scrum';
export type TeamRole = 'admin' | 'member' | 'viewer';
export type SprintStatus = 'planned' | 'active' | 'completed';

// ─── Core entities ───────────────────────────────────────────────────

export type BoardUser = {
  id: string;
  email: string;
  displayName: string;
  avatarUrl?: string;
}

export type Team = {
  id: string;
  name: string;
  description: string;
  inviteCode: string;
  createdBy: string;
  createdAt: string;
  updatedAt: string;
}

export type TeamMember = {
  id: string;
  teamId: string;
  userId: string;
  role: TeamRole;
  joinedAt: string;
  user?: BoardUser;
}

export type Board = {
  id: string;
  teamId: string;
  name: string;
  key: string;
  description: string;
  boardType: BoardType;
  issueCounter: number;
  createdAt: string;
  updatedAt: string;
}

export type BoardColumn = {
  id: string;
  boardId: string;
  name: string;
  color: string;
  position: number;
  wipLimit?: number;
}

export type Sprint = {
  id: string;
  boardId: string;
  name: string;
  goal: string;
  startDate: string;
  endDate: string;
  status: SprintStatus;
  createdAt: string;
}

export type Issue = {
  id: string;
  boardId: string;
  columnId: string;
  sprintId?: string;
  parentId?: string;
  issueKey: string;
  issueType: IssueType;
  title: string;
  description: string;
  priority: Priority;
  assigneeId?: string;
  reporterId: string;
  storyPoints?: number;
  dueDate?: string;
  gitBranch?: string;
  position: number;
  labels: Label[];
  assignee?: BoardUser;
  reporter?: BoardUser;
  subtaskCount?: number;
  commentCount?: number;
  createdAt: string;
  updatedAt: string;
}

export type Comment = {
  id: string;
  issueId: string;
  authorId: string;
  body: string;
  author?: BoardUser;
  createdAt: string;
  updatedAt: string;
}

export type ActivityLog = {
  id: string;
  issueId: string;
  userId: string;
  action: string;
  field: string;
  oldValue: string;
  newValue: string;
  user?: BoardUser;
  createdAt: string;
}

export type Label = {
  id: string;
  boardId: string;
  name: string;
  color: string;
}

// ─── Input types for create / update operations ─────────────────────

export type CreateTeamInput = {
  name: string;
  description?: string;
}

export type CreateBoardInput = {
  name: string;
  key: string;
  description?: string;
  boardType: BoardType;
}

export type CreateIssueInput = {
  title: string;
  description?: string;
  issueType: IssueType;
  priority: Priority;
  columnId: string;
  sprintId?: string;
  parentId?: string;
  assigneeId?: string;
  storyPoints?: number;
  dueDate?: string;
  gitBranch?: string;
  labelIds?: string[];
}

export type UpdateIssueInput = {
  title?: string;
  description?: string;
  priority?: Priority;
  assigneeId?: string | null;
  sprintId?: string | null;
  storyPoints?: number | null;
  dueDate?: string | null;
  gitBranch?: string | null;
}

export type MoveIssueInput = {
  columnId: string;
  position: number;
}

export type CreateCommentInput = {
  body: string;
}

export type CreateSprintInput = {
  name: string;
  goal?: string;
  startDate: string;
  endDate: string;
}

export type CreateLabelInput = {
  name: string;
  color: string;
}

// ─── WebSocket event types ──────────────────────────────────────────

export type WsEventType =
  | 'issue_created'
  | 'issue_updated'
  | 'issue_moved'
  | 'issue_deleted'
  | 'comment_added'
  | 'comment_updated'
  | 'member_joined'
  | 'member_left'
  | 'sprint_started'
  | 'sprint_completed'
  | 'column_reordered';

export type WsEvent = {
  type: WsEventType;
  boardId: string;
  payload: unknown;
  userId: string;
  timestamp: string;
}
