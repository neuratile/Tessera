import { create } from 'zustand';

// ── Type imports ──────────────────────────────────────────────────────
// These mirror the shared package types. Once the shared package is
// updated they can be replaced with `@testing-ide/shared` imports.

export type IssueType = 'epic' | 'story' | 'task' | 'bug' | 'subtask';
export type Priority = 'critical' | 'high' | 'medium' | 'low' | 'trivial';
export type BoardType = 'kanban' | 'scrum';
export type TeamRole = 'admin' | 'member' | 'viewer';
export type SprintStatus = 'planned' | 'active' | 'completed';

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

export type Label = {
  id: string;
  boardId: string;
  name: string;
  color: string;
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

// ── Store State ───────────────────────────────────────────────────────

type BoardStoreState = {
  // Connection
  serverUrl: string | null;
  connected: boolean;
  connecting: boolean;
  connectionError: string | null;

  // Current user on the server
  currentUser: BoardUser | null;

  // Active context
  activeTeamId: string | null;
  activeBoardId: string | null;
  activeIssueId: string | null;
  draggedIssueId: string | null;

  // Data collections
  teams: Team[];
  members: TeamMember[];
  boards: Board[];
  columns: BoardColumn[];
  issues: Issue[];
  sprints: Sprint[];
  labels: Label[];
  comments: Comment[];
  activities: ActivityLog[];

  // Loading states
  loadingTeams: boolean;
  loadingBoard: boolean;
  loadingIssues: boolean;

  // Filter state
  filterAssignee: string | null;
  filterPriority: Priority | null;
  filterType: IssueType | null;
  filterSprint: string | null;
  searchQuery: string;

  // Actions — connection
  setServerUrl: (url: string) => void;
  setConnected: (connected: boolean) => void;
  setConnecting: (connecting: boolean) => void;
  setConnectionError: (error: string | null) => void;
  setCurrentUser: (user: BoardUser | null) => void;

  // Actions — navigation
  setActiveTeam: (teamId: string | null) => void;
  setActiveBoard: (boardId: string | null) => void;
  setActiveIssue: (issueId: string | null) => void;

  // Actions — data setters
  setTeams: (teams: Team[]) => void;
  setMembers: (members: TeamMember[]) => void;
  setBoards: (boards: Board[]) => void;
  setColumns: (columns: BoardColumn[]) => void;
  setIssues: (issues: Issue[]) => void;
  setSprints: (sprints: Sprint[]) => void;
  setLabels: (labels: Label[]) => void;
  setComments: (comments: Comment[]) => void;
  setActivities: (activities: ActivityLog[]) => void;

  // Actions — loading
  setLoadingTeams: (loading: boolean) => void;
  setLoadingBoard: (loading: boolean) => void;
  setLoadingIssues: (loading: boolean) => void;

  // Actions — optimistic updates (for real-time)
  addIssue: (issue: Issue) => void;
  updateIssue: (issueId: string, updates: Partial<Issue>) => void;
  removeIssue: (issueId: string) => void;
  moveIssueOptimistic: (issueId: string, columnId: string, position: number) => void;
  addComment: (comment: Comment) => void;
  addMember: (member: TeamMember) => void;
  removeMember: (memberId: string) => void;

  // Actions — filters
  setFilterAssignee: (id: string | null) => void;
  setFilterPriority: (priority: Priority | null) => void;
  setFilterType: (type: IssueType | null) => void;
  setFilterSprint: (sprintId: string | null) => void;
  setSearchQuery: (query: string) => void;
  clearFilters: () => void;

  // Actions — reset
  reset: () => void;

  // Drag-and-drop support
  setDraggedIssueId: (issueId: string | null) => void;
};

const initialState = {
  serverUrl: null as string | null,
  connected: false,
  connecting: false,
  connectionError: null as string | null,
  currentUser: null as BoardUser | null,
  activeTeamId: null as string | null,
  activeBoardId: null as string | null,
  activeIssueId: null as string | null,
  draggedIssueId: null as string | null,
  teams: [] as Team[],
  members: [] as TeamMember[],
  boards: [] as Board[],
  columns: [] as BoardColumn[],
  issues: [] as Issue[],
  sprints: [] as Sprint[],
  labels: [] as Label[],
  comments: [] as Comment[],
  activities: [] as ActivityLog[],
  loadingTeams: false,
  loadingBoard: false,
  loadingIssues: false,
  filterAssignee: null as string | null,
  filterPriority: null as Priority | null,
  filterType: null as IssueType | null,
  filterSprint: null as string | null,
  searchQuery: '',
};

const store = create<BoardStoreState>((set) => ({
  ...initialState,

  // Connection
  setServerUrl: (url) => set({ serverUrl: url }),
  setConnected: (connected) => set({ connected }),
  setConnecting: (connecting) => set({ connecting }),
  setConnectionError: (error) => set({ connectionError: error }),
  setCurrentUser: (user) => set({ currentUser: user }),
  setDraggedIssueId: (id) => set({ draggedIssueId: id }),

  // Navigation
  setActiveTeam: (teamId) =>
    set({ activeTeamId: teamId, activeBoardId: null, activeIssueId: null }),
  setActiveBoard: (boardId) => set({ activeBoardId: boardId, activeIssueId: null }),
  setActiveIssue: (issueId) => set({ activeIssueId: issueId }),

  // Data setters
  setTeams: (teams) => set({ teams }),
  setMembers: (members) => set({ members }),
  setBoards: (boards) => set({ boards }),
  setColumns: (columns) => set({ columns }),
  setIssues: (issues) => set({ issues }),
  setSprints: (sprints) => set({ sprints }),
  setLabels: (labels) => set({ labels }),
  setComments: (comments) => set({ comments }),
  setActivities: (activities) => set({ activities }),

  // Loading
  setLoadingTeams: (loading) => set({ loadingTeams: loading }),
  setLoadingBoard: (loading) => set({ loadingBoard: loading }),
  setLoadingIssues: (loading) => set({ loadingIssues: loading }),

  // Optimistic updates
  addIssue: (issue) => set((s) => ({ issues: [...s.issues, issue] })),

  updateIssue: (issueId, updates) =>
    set((s) => ({
      issues: s.issues.map((i) => (i.id === issueId ? { ...i, ...updates } : i)),
    })),

  removeIssue: (issueId) =>
    set((s) => ({ issues: s.issues.filter((i) => i.id !== issueId) })),

  moveIssueOptimistic: (issueId, columnId, position) =>
    set((s) => {
      const issue = s.issues.find((i) => i.id === issueId);
      if (!issue) return {};

      const oldColId = issue.columnId;
      const oldPos = issue.position;

      const updatedIssues = s.issues.map((i) => {
        if (i.id === issueId) {
          return { ...i, columnId, position };
        }

        if (oldColId === columnId) {
          // Shifting within same column
          if (oldPos < position) {
            // Moving down: shift items in between up (decrement position)
            if (i.columnId === oldColId && i.position > oldPos && i.position <= position) {
              return { ...i, position: i.position - 1 };
            }
          } else if (oldPos > position) {
            // Moving up: shift items in between down (increment position)
            if (i.columnId === oldColId && i.position >= position && i.position < oldPos) {
              return { ...i, position: i.position + 1 };
            }
          }
        } else {
          // Shifting across columns
          // 1. Shift old column items down (decrement position)
          if (i.columnId === oldColId && i.position > oldPos) {
            return { ...i, position: i.position - 1 };
          }
          // 2. Shift new column items up (increment position)
          if (i.columnId === columnId && i.position >= position) {
            return { ...i, position: i.position + 1 };
          }
        }

        return i;
      });

      return { issues: updatedIssues };
    }),

  addComment: (comment) => set((s) => ({ comments: [...s.comments, comment] })),

  addMember: (member) => set((s) => ({ members: [...s.members, member] })),

  removeMember: (memberId) =>
    set((s) => ({ members: s.members.filter((m) => m.id !== memberId) })),

  // Filters
  setFilterAssignee: (id) => set({ filterAssignee: id }),
  setFilterPriority: (priority) => set({ filterPriority: priority }),
  setFilterType: (type) => set({ filterType: type }),
  setFilterSprint: (sprintId) => set({ filterSprint: sprintId }),
  setSearchQuery: (query) => set({ searchQuery: query }),
  clearFilters: () =>
    set({
      filterAssignee: null,
      filterPriority: null,
      filterType: null,
      filterSprint: null,
      searchQuery: '',
    }),

  // Reset
  reset: () => set({ ...initialState }),
}));

// HMR-safe singleton pattern (matches other stores)
const globalStore = globalThis as unknown as { useBoardStore?: typeof store };
export const useBoardStore = globalStore.useBoardStore || store;
if (process.env.NODE_ENV !== 'production') {
  globalStore.useBoardStore = useBoardStore;
}
