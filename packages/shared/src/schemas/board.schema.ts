import { z } from 'zod';

// ─── Enum schemas ────────────────────────────────────────────────────

export const IssueTypeSchema = z.enum(['epic', 'story', 'task', 'bug', 'subtask']);
export const PrioritySchema = z.enum(['critical', 'high', 'medium', 'low', 'trivial']);
export const BoardTypeSchema = z.enum(['kanban', 'scrum']);
export const TeamRoleSchema = z.enum(['admin', 'member', 'viewer']);
export const SprintStatusSchema = z.enum(['planned', 'active', 'completed']);

// ─── Helpers ─────────────────────────────────────────────────────────

const IsoDateTimeSchema = z.string().datetime({ offset: true });

// ─── Core entity schemas ─────────────────────────────────────────────

export const BoardUserSchema = z.object({
  id: z.string().uuid(),
  email: z.string().email(),
  displayName: z.string().min(1),
  avatarUrl: z.string().url().optional(),
});

export const TeamSchema = z.object({
  id: z.string().uuid(),
  name: z.string().min(1),
  description: z.string().optional(),
  inviteCode: z.string().min(1),
  createdBy: z.string().uuid(),
  createdAt: IsoDateTimeSchema,
  updatedAt: IsoDateTimeSchema,
});

export const TeamMemberSchema = z.object({
  id: z.string().uuid(),
  teamId: z.string().uuid(),
  userId: z.string().uuid(),
  role: TeamRoleSchema,
  joinedAt: IsoDateTimeSchema,
  user: BoardUserSchema.optional(),
});

export const BoardSchema = z.object({
  id: z.string().uuid(),
  teamId: z.string().uuid(),
  name: z.string().min(1),
  key: z.string().min(1),
  description: z.string().optional(),
  boardType: BoardTypeSchema,
  issueCounter: z.number().int().nonnegative(),
  createdAt: IsoDateTimeSchema,
  updatedAt: IsoDateTimeSchema,
});

export const BoardColumnSchema = z.object({
  id: z.string().uuid(),
  boardId: z.string().uuid(),
  name: z.string().min(1),
  color: z.string().min(1),
  position: z.number().int().nonnegative(),
  wipLimit: z.number().int().positive().optional(),
});

export const SprintSchema = z.object({
  id: z.string().uuid(),
  boardId: z.string().uuid(),
  name: z.string().min(1),
  goal: z.string().optional(),
  startDate: IsoDateTimeSchema.optional(),
  endDate: IsoDateTimeSchema.optional(),
  status: SprintStatusSchema,
  createdAt: IsoDateTimeSchema,
});

export const LabelSchema = z.object({
  id: z.string().uuid(),
  boardId: z.string().uuid(),
  name: z.string().min(1),
  color: z.string().min(1),
});

export const IssueSchema = z.object({
  id: z.string().uuid(),
  boardId: z.string().uuid(),
  columnId: z.string().uuid(),
  sprintId: z.string().uuid().optional(),
  parentId: z.string().uuid().optional(),
  issueKey: z.string().min(1),
  issueType: IssueTypeSchema,
  title: z.string().min(1),
  description: z.string(),
  priority: PrioritySchema,
  assigneeId: z.string().uuid().optional(),
  reporterId: z.string().uuid(),
  storyPoints: z.number().int().nonnegative().optional(),
  dueDate: IsoDateTimeSchema.optional(),
  gitBranch: z.string().optional(),
  position: z.number().int().nonnegative(),
  labels: z.array(LabelSchema),
  assignee: BoardUserSchema.optional(),
  reporter: BoardUserSchema.optional(),
  subtaskCount: z.number().int().nonnegative().optional(),
  commentCount: z.number().int().nonnegative().optional(),
  createdAt: IsoDateTimeSchema,
  updatedAt: IsoDateTimeSchema,
});

export const CommentSchema = z.object({
  id: z.string().uuid(),
  issueId: z.string().uuid(),
  authorId: z.string().uuid(),
  body: z.string().min(1),
  author: BoardUserSchema.optional(),
  createdAt: IsoDateTimeSchema,
  updatedAt: IsoDateTimeSchema,
});

export const ActivityLogSchema = z.object({
  id: z.string().uuid(),
  issueId: z.string().uuid(),
  userId: z.string().uuid(),
  action: z.string().min(1),
  field: z.string().optional(),
  oldValue: z.string().optional(),
  newValue: z.string().optional(),
  user: BoardUserSchema.optional(),
  createdAt: IsoDateTimeSchema,
});

// ─── Input schemas ───────────────────────────────────────────────────

export const CreateTeamInputSchema = z.object({
  name: z.string().min(1).max(100),
  description: z.string().max(500).optional(),
});

export const CreateBoardInputSchema = z.object({
  name: z.string().min(1).max(100),
  key: z.string().min(2).max(10).regex(/^[A-Z][A-Z0-9]*$/, 'Board key must be uppercase alphanumeric'),
  description: z.string().max(500).optional(),
  boardType: BoardTypeSchema,
});

export const CreateIssueInputSchema = z.object({
  title: z.string().min(1).max(500),
  description: z.string().max(10000).optional(),
  issueType: IssueTypeSchema,
  priority: PrioritySchema,
  columnId: z.string().uuid(),
  sprintId: z.string().uuid().optional(),
  parentId: z.string().uuid().optional(),
  assigneeId: z.string().uuid().optional(),
  storyPoints: z.number().int().nonnegative().optional(),
  dueDate: IsoDateTimeSchema.optional(),
  gitBranch: z.string().optional(),
  labelIds: z.array(z.string().uuid()).optional(),
});

export const UpdateIssueInputSchema = z.object({
  title: z.string().min(1).max(500).optional(),
  description: z.string().max(10000).optional(),
  priority: PrioritySchema.optional(),
  assigneeId: z.string().uuid().nullable().optional(),
  sprintId: z.string().uuid().nullable().optional(),
  storyPoints: z.number().int().nonnegative().nullable().optional(),
  dueDate: IsoDateTimeSchema.nullable().optional(),
  gitBranch: z.string().nullable().optional(),
});

export const MoveIssueInputSchema = z.object({
  columnId: z.string().uuid(),
  position: z.number().int().nonnegative(),
});

export const CreateCommentInputSchema = z.object({
  body: z.string().min(1).max(10000),
});

export const CreateSprintInputSchema = z.object({
  name: z.string().min(1).max(100),
  goal: z.string().max(500).optional(),
  startDate: IsoDateTimeSchema,
  endDate: IsoDateTimeSchema,
});

export const CreateLabelInputSchema = z.object({
  name: z.string().min(1).max(50),
  color: z.string().regex(/^#[0-9a-fA-F]{6}$/, 'Color must be a hex color code'),
});

// ─── WebSocket event schemas ─────────────────────────────────────────

export const WsEventTypeSchema = z.enum([
  'issue_created',
  'issue_updated',
  'issue_moved',
  'issue_deleted',
  'comment_added',
  'comment_updated',
  'member_joined',
  'member_left',
  'sprint_started',
  'sprint_completed',
  'column_reordered',
]);

export const WsEventSchema = z.object({
  type: WsEventTypeSchema,
  boardId: z.string().uuid(),
  payload: z.unknown(),
  userId: z.string().uuid(),
  timestamp: IsoDateTimeSchema,
});
