import {
  Bug,
  CheckCircle2,
  ClipboardList,
  FileBarChart,
  FileText,
  FolderOpen,
  Github,
  PanelLeftClose,
  PanelRightClose,
  RefreshCw,
  Search,
  Settings,
  Sparkles,
  Workflow,
} from 'lucide-react';
import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactElement,
} from 'react';

import { COMMAND, dispatchCommand, type CommandId } from '@/lib/command-bus';

/**
 * Command palette.
 *
 * VS Code / Linear-style modal launched by `Cmd/Ctrl+K`. Lists every
 * routable action in the desktop — analyze, generate per artifact
 * type, toggle panels, open settings, jump to project, open docs.
 * Each row dispatches into the same `commandBus` the native menu
 * uses, so feature components own their own actions and the palette
 * never re-implements an IPC flow.
 *
 * Keyboard contract (inside the palette):
 *   Up / Down       move highlight
 *   PageUp / PageDown jump by 5
 *   Home / End      first / last match
 *   Enter           run highlighted command, close
 *   Esc             close without running
 *
 * Outside the palette the open / close is handled by the
 * `command-palette/open` command on the bus (`Cmd/Ctrl+K`).
 */

type CommandEntry = {
  id: string;
  label: string;
  /** Optional secondary text — keybind hint, "(active)" marker, etc. */
  hint?: string;
  /** Search keywords beyond `label`. Lowercase only. */
  aliases?: readonly string[];
  group: 'AI' | 'Workspace' | 'View' | 'Help';
  icon: ReactElement;
  /**
   * Either dispatch a bus command (by id) or run an arbitrary
   * callback. Both close the palette on completion.
   */
  action: { kind: 'bus'; command: CommandId } | { kind: 'fn'; run: () => void };
};

const COMMANDS: ReadonlyArray<CommandEntry> = [
  {
    id: 'ai.analyze',
    label: 'Analyze Project',
    hint: 'Ctrl+Shift+A',
    aliases: ['index', 'embed', 'chunk', 'scan'],
    group: 'AI',
    icon: <Workflow className="size-3.5" />,
    action: { kind: 'bus', command: COMMAND.AiAnalyze },
  },
  {
    id: 'ai.regenerate',
    label: 'Regenerate Last Artifact',
    hint: 'Ctrl+G',
    aliases: ['retry', 'redo', 'rerun'],
    group: 'AI',
    icon: <RefreshCw className="size-3.5" />,
    action: { kind: 'bus', command: COMMAND.AiRegenerate },
  },
  {
    id: 'ai.generate.context',
    label: 'Generate Context',
    aliases: ['context', 'overview', 'summary'],
    group: 'AI',
    icon: <FileText className="size-3.5" />,
    action: { kind: 'fn', run: () => fireGenerate('context-md') },
  },
  {
    id: 'ai.generate.test-plan',
    label: 'Generate Test Plan',
    aliases: ['plan', 'strategy'],
    group: 'AI',
    icon: <ClipboardList className="size-3.5" />,
    action: { kind: 'fn', run: () => fireGenerate('test-plan') },
  },
  {
    id: 'ai.generate.test-cases',
    label: 'Generate Test Cases',
    aliases: ['cases', 'scenarios'],
    group: 'AI',
    icon: <CheckCircle2 className="size-3.5" />,
    action: { kind: 'fn', run: () => fireGenerate('test-cases') },
  },
  {
    id: 'ai.generate.defects',
    label: 'Generate Defect Report',
    aliases: ['defects', 'issues', 'static analysis'],
    group: 'AI',
    icon: <Bug className="size-3.5" />,
    action: { kind: 'fn', run: () => fireGenerate('defect-report') },
  },
  {
    id: 'ai.generate.bugs',
    label: 'Generate Bug Report',
    aliases: ['bugs', 'runtime'],
    group: 'AI',
    icon: <FileBarChart className="size-3.5" />,
    action: { kind: 'fn', run: () => fireGenerate('bug-report') },
  },
  {
    id: 'workspace.open-folder',
    label: 'Open Folder…',
    hint: 'Ctrl+O',
    aliases: ['project', 'directory'],
    group: 'Workspace',
    icon: <FolderOpen className="size-3.5" />,
    action: { kind: 'bus', command: COMMAND.FileOpenFolder },
  },
  {
    id: 'workspace.settings',
    label: 'Settings',
    hint: 'Ctrl+,',
    aliases: ['provider', 'preferences', 'config'],
    group: 'Workspace',
    icon: <Settings className="size-3.5" />,
    action: { kind: 'bus', command: COMMAND.FileSettings },
  },
  {
    id: 'view.toggle-sidebar',
    label: 'Toggle Sidebar',
    hint: 'Ctrl+B',
    aliases: ['explorer', 'tree'],
    group: 'View',
    icon: <PanelLeftClose className="size-3.5" />,
    action: { kind: 'bus', command: COMMAND.ViewToggleSidebar },
  },
  {
    id: 'view.toggle-ai-panel',
    label: 'Toggle AI Panel',
    hint: 'Ctrl+J',
    aliases: ['inspect', 'right'],
    group: 'View',
    icon: <PanelRightClose className="size-3.5" />,
    action: { kind: 'bus', command: COMMAND.ViewToggleAiPanel },
  },
  {
    id: 'help.docs',
    label: 'Open Documentation',
    aliases: ['readme', 'guide'],
    group: 'Help',
    icon: <Sparkles className="size-3.5" />,
    action: { kind: 'bus', command: COMMAND.HelpDocs },
  },
  {
    id: 'help.github',
    label: 'Open GitHub Repository',
    hint: 'Ctrl+Shift+G',
    aliases: ['source', 'repo'],
    group: 'Help',
    icon: <Github className="size-3.5" />,
    action: { kind: 'bus', command: COMMAND.HelpGithub },
  },
];

/**
 * Dispatch a generator request via the AI panel's existing action
 * surface. Mirrors the GENERATE_BUTTONS array in `ai-panel.tsx` so
 * the palette doesn't re-implement IPC orchestration — it fires a
 * `palette/generate` window event the panel listens for.
 */
function fireGenerate(artifactType: string): void {
  window.dispatchEvent(
    new CustomEvent('palette:generate', { detail: artifactType }),
  );
}

type Props = {
  open: boolean;
  onClose: () => void;
};

export function CommandPalette({ open, onClose }: Props) {
  const [query, setQuery] = useState('');
  const [highlight, setHighlight] = useState(0);
  const inputRef = useRef<HTMLInputElement | null>(null);

  // Scope: when the palette opens, reset the query and focus the
  // input. Restore focus to the previously-focused element on close
  // (Tauri windows lose focus quickly without this).
  useEffect(() => {
    if (!open) return;
    const active = document.activeElement;
    const previouslyFocused = active instanceof HTMLElement ? active : null;
    setQuery('');
    setHighlight(0);
    // Microtask defer so the input renders before we focus it.
    queueMicrotask(() => inputRef.current?.focus());
    return () => {
      previouslyFocused?.focus();
    };
  }, [open]);

  const matches = useMemo(() => {
    const needle = query.trim().toLowerCase();
    if (needle.length === 0) return COMMANDS;
    return COMMANDS.filter((c) => {
      if (c.label.toLowerCase().includes(needle)) return true;
      if (c.aliases?.some((a) => a.includes(needle))) return true;
      if (c.group.toLowerCase().includes(needle)) return true;
      return false;
    });
  }, [query]);

  // Clamp highlight when matches shrink — without this, deleting
  // characters while a late row was selected leaves the highlight
  // pointing past the end of the array.
  useEffect(() => {
    if (highlight >= matches.length) setHighlight(matches.length === 0 ? 0 : matches.length - 1);
  }, [matches.length, highlight]);

  const runAt = useCallback(
    (index: number) => {
      const entry = matches[index];
      if (entry === undefined) return;
      if (entry.action.kind === 'bus') {
        dispatchCommand(entry.action.command);
      } else {
        entry.action.run();
      }
      onClose();
    },
    [matches, onClose],
  );

  if (!open) return null;

  return (
    <>
      <div
        className="bg-background/80 fixed inset-0 z-50 backdrop-blur-sm"
        onClick={onClose}
        aria-hidden="true"
      />
      <div
        role="dialog"
        aria-modal="true"
        aria-label="Command palette"
        className="fixed inset-x-0 top-[14vh] z-50 mx-auto flex w-full max-w-xl flex-col overflow-hidden rounded-lg border border-border bg-card shadow-2xl"
        onKeyDown={(event) => {
          if (event.key === 'Escape') {
            event.stopPropagation();
            onClose();
            return;
          }
          if (event.key === 'Enter') {
            event.preventDefault();
            runAt(highlight);
            return;
          }
          if (event.key === 'ArrowDown') {
            event.preventDefault();
            setHighlight((h) => Math.min(matches.length - 1, h + 1));
            return;
          }
          if (event.key === 'ArrowUp') {
            event.preventDefault();
            setHighlight((h) => Math.max(0, h - 1));
            return;
          }
          if (event.key === 'PageDown') {
            event.preventDefault();
            setHighlight((h) => Math.min(matches.length - 1, h + 5));
            return;
          }
          if (event.key === 'PageUp') {
            event.preventDefault();
            setHighlight((h) => Math.max(0, h - 5));
            return;
          }
          if (event.key === 'Home') {
            event.preventDefault();
            setHighlight(0);
            return;
          }
          if (event.key === 'End') {
            event.preventDefault();
            setHighlight(Math.max(0, matches.length - 1));
            return;
          }
        }}
      >
        <div className="relative border-b border-border">
          <Search className="text-muted-foreground absolute left-3 top-1/2 size-4 -translate-y-1/2" />
          <input
            ref={inputRef}
            type="text"
            value={query}
            onChange={(e) => {
              setQuery(e.target.value);
              setHighlight(0);
            }}
            placeholder="Type a command, file, or generator…"
            aria-label="Command query"
            className="bg-transparent placeholder:text-muted-foreground h-10 w-full px-10 text-sm focus:outline-none"
          />
          <kbd className="text-muted-foreground border-border absolute right-3 top-1/2 hidden -translate-y-1/2 rounded border px-1 font-mono text-[10px] sm:inline">
            Esc
          </kbd>
        </div>

        <ul role="listbox" className="max-h-[55vh] overflow-y-auto p-1">
          {matches.length === 0 ? (
            <li className="text-muted-foreground px-3 py-6 text-center text-xs">
              No commands match <code className="font-mono">{query}</code>.
            </li>
          ) : (
            matches.map((entry, index) => {
              const isHighlighted = index === highlight;
              return (
                <li key={entry.id}>
                  <button
                    type="button"
                    role="option"
                    aria-selected={isHighlighted}
                    onMouseMove={() => {
                      if (!isHighlighted) setHighlight(index);
                    }}
                    onClick={() => runAt(index)}
                    className={`flex w-full items-center gap-3 rounded-md px-3 py-2 text-left text-xs transition-colors ${
                      isHighlighted ? 'bg-primary/10 text-primary' : 'text-foreground'
                    }`}
                  >
                    <span
                      className={isHighlighted ? 'text-primary' : 'text-muted-foreground'}
                    >
                      {entry.icon}
                    </span>
                    <span className="flex-1 truncate">{entry.label}</span>
                    <span className="text-muted-foreground text-[10px] font-semibold uppercase tracking-[0.08em]">
                      {entry.group}
                    </span>
                    {entry.hint !== undefined ? (
                      <kbd className="text-muted-foreground border-border rounded border px-1 font-mono text-[10px]">
                        {entry.hint}
                      </kbd>
                    ) : null}
                  </button>
                </li>
              );
            })
          )}
        </ul>
      </div>
    </>
  );
}
