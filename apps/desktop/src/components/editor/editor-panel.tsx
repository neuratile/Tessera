import Editor, { loader, type OnMount } from '@monaco-editor/react';
import type { CoverageLine } from '@testing-ide/shared';
import { Circle, X } from 'lucide-react';
import * as monaco from 'monaco-editor';
import { useCallback, useEffect, useMemo, useRef } from 'react';

import { languageFromFilename } from '@/lib/monaco-language';
import { usePrefersDark } from '@/lib/theme';
import { useEditorStore } from '@/stores/editor-store';
import { useSandboxStore } from '@/stores/sandbox-store';

/**
 * Match a run's coverage lines to the open file. The runner reports
 * workspace-relative (or `/work/…`) paths, the editor keys tabs by
 * project-relative path; normalize both and match by suffix so e.g.
 * `/work/src/add.ts` lines paint onto an open `src/add.ts`.
 */
function coverageForPath(coverage: CoverageLine[], relativePath: string): CoverageLine[] {
  const norm = (p: string): string => p.replace(/^\/?work\//, '').replace(/^\.?\//, '');
  const target = norm(relativePath);
  return coverage.filter((c) => {
    const cp = norm(c.filePath);
    return cp === target || cp.endsWith(`/${target}`) || target.endsWith(`/${cp}`);
  });
}

/**
 * Tabbed Monaco editor.
 *
 * Phase 10 ships read-only viewing. The buffer is editable in-memory
 * (Monaco's default), and `markDirty` flips the tab indicator, but
 * persistence to disk is deferred — there is no save action yet. We
 * deliberately avoid `readOnly: true` so the user can still scratch /
 * try edits while the AI panel runs.
 *
 * `loader.config({ monaco })` switches `@monaco-editor/react` from its
 * default CDN load to the locally bundled `monaco-editor` package. The
 * desktop CSP forbids cross-origin scripts; this also keeps the editor
 * functional offline.
 */
loader.config({ monaco });

export function EditorPanel() {
  const tabs = useEditorStore((s) => s.tabs);
  const activeId = useEditorStore((s) => s.activeId);
  const contents = useEditorStore((s) => s.contents);
  const errors = useEditorStore((s) => s.errors);
  const loading = useEditorStore((s) => s.loading);
  const setActive = useEditorStore((s) => s.setActive);
  const closeTab = useEditorStore((s) => s.closeTab);
  const setContent = useEditorStore((s) => s.setContent);
  const markDirty = useEditorStore((s) => s.markDirty);

  const coverage = useSandboxStore((s) => s.coverage);

  const dark = usePrefersDark();
  const activeTab = useMemo(
    () => tabs.find((t) => t.id === activeId) ?? null,
    [tabs, activeId],
  );

  // Monaco instance + its coverage-decoration collection, captured on mount
  // so the effect below can repaint gutters as the run results or the open
  // file change without remounting the editor.
  const editorRef = useRef<monaco.editor.IStandaloneCodeEditor | null>(null);
  const decorationsRef = useRef<monaco.editor.IEditorDecorationsCollection | null>(null);

  const handleMount = useCallback<OnMount>((editor) => {
    editorRef.current = editor;
  }, []);

  useEffect(() => {
    const ed = editorRef.current;
    if (ed === null) return;
    if (activeTab === null) {
      decorationsRef.current?.clear();
      return;
    }
    const lines = coverageForPath(coverage, activeTab.relativePath);
    const decos = lines.map((c) => ({
      range: new monaco.Range(c.line, 1, c.line, 1),
      options: {
        linesDecorationsClassName: c.hits > 0 ? 'cov-hit' : 'cov-miss',
        hoverMessage: { value: c.hits > 0 ? `Covered — ${c.hits} hit(s)` : 'Not covered' },
      },
    }));
    if (decorationsRef.current === null) {
      decorationsRef.current = ed.createDecorationsCollection(decos);
    } else {
      decorationsRef.current.set(decos);
    }
  }, [coverage, activeTab]);

  const handleChange = useCallback(
    (value: string | undefined) => {
      if (activeTab === null) return;
      const next = value ?? '';
      const previous = contents[activeTab.relativePath];
      if (previous === next) return;
      setContent(activeTab.relativePath, next);
      // Initial load fires `onChange` once — only mark dirty when the
      // user actually diverges from the on-disk read.
      if (previous !== undefined) {
        markDirty(activeTab.relativePath, true);
      }
    },
    [activeTab, contents, setContent, markDirty],
  );

  if (tabs.length === 0 || activeTab === null) {
    return <EmptyState />;
  }

  const error = errors[activeTab.relativePath];
  const isLoading = loading[activeTab.relativePath] === true;
  const value = contents[activeTab.relativePath] ?? '';
  const language = languageFromFilename(activeTab.name);

  return (
    <div className="flex h-full flex-col bg-background">
      <TabStrip />
      <div className="relative flex-1">
        {error !== undefined ? (
          <div className="absolute inset-0 flex items-center justify-center px-6 text-center">
            <div className="max-w-md">
              <p className="text-destructive text-sm" role="alert">
                {error}
              </p>
              <p className="text-muted-foreground mt-2 text-xs">
                Close this tab and pick another file.
              </p>
            </div>
          </div>
        ) : isLoading && value === '' ? (
          <p className="text-muted-foreground flex h-full items-center justify-center text-sm">
            Reading file…
          </p>
        ) : (
          <Editor
            height="100%"
            language={language}
            theme={dark ? 'vs-dark' : 'light'}
            value={value}
            onChange={handleChange}
            onMount={handleMount}
            options={{
              minimap: { enabled: false },
              fontSize: 13,
              fontFamily: '"Cascadia Code", "JetBrains Mono", Consolas, monospace',
              lineNumbersMinChars: 3,
              wordWrap: 'on',
              scrollBeyondLastLine: false,
              smoothScrolling: true,
              cursorBlinking: 'smooth',
              renderWhitespace: 'selection',
              tabSize: 2,
            }}
            loading={
              <p className="text-muted-foreground flex h-full items-center justify-center text-sm">
                Loading editor…
              </p>
            }
          />
        )}
      </div>
    </div>
  );

  function TabStrip() {
    return (
      // Tab strip uses ARIA tablist semantics so the close button
      // can sit inside each tab as a real `<button>`. Outer is a
      // `<div role="tab">` (not a `<button>`) because nesting two
      // `<button>` elements is invalid HTML — the parent button
      // would swallow `Enter` events the close button should own.
      <div
        role="tablist"
        aria-label="Open files"
        className="bg-muted/30 flex shrink-0 overflow-x-auto border-b border-border"
      >
        {tabs.map((tab) => {
          const isActive = tab.id === activeId;
          return (
            <div
              key={tab.id}
              role="tab"
              tabIndex={isActive ? 0 : -1}
              aria-selected={isActive}
              onClick={() => setActive(tab.id)}
              onKeyDown={(e) => {
                if (e.key === 'Enter' || e.key === ' ') {
                  e.preventDefault();
                  setActive(tab.id);
                }
              }}
              className={`group flex min-w-fit max-w-[220px] cursor-pointer items-center gap-2 border-r border-border px-3 py-1.5 text-xs transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-primary/40 ${
                isActive
                  ? 'border-t-primary bg-background text-foreground border-t-2'
                  : 'text-muted-foreground hover:bg-muted/50 border-t-2 border-t-transparent'
              }`}
              title={tab.relativePath}
            >
              <span className="truncate">{tab.name}</span>
              <button
                type="button"
                aria-label={`Close ${tab.name}`}
                onClick={(e) => {
                  e.stopPropagation();
                  closeTab(tab.id);
                }}
                onKeyDown={(e) => {
                  // Stop the parent tab's Enter / Space handler from
                  // re-activating the tab when the user closes it
                  // via the keyboard.
                  if (e.key === 'Enter' || e.key === ' ') {
                    e.stopPropagation();
                  }
                }}
                className="text-muted-foreground hover:bg-muted hover:text-foreground rounded p-0.5"
              >
                {tab.dirty ? (
                  <Circle className="text-primary size-3 fill-current" />
                ) : (
                  <X className="size-3" />
                )}
              </button>
            </div>
          );
        })}
      </div>
    );
  }
}

function EmptyState() {
  // Stitch empty-state pattern — centred copy over the mosaic
  // watermark, logo + monospace brand mark to reinforce the IDE feel.
  return (
    <div className="relative flex flex-1 flex-col items-center justify-center p-8 text-center">
      <div className="bg-mosaic" aria-hidden="true" />
      <div className="relative z-10 flex flex-col items-center">
        <img
          src="/tessera-logo.png"
          alt=""
          aria-hidden="true"
          className="mb-3 size-16 rounded-lg opacity-80"
          draggable="false"
        />
        <span className="font-brand text-primary/70 text-xl">tessera</span>
        <h2 className="mt-3 text-base font-semibold tracking-tight text-foreground">
          No file open
        </h2>
        <p className="text-muted-foreground mt-1 max-w-md text-xs">
          Pick a file from the explorer to view it. Saving lands in a later phase — edits stay
          in-memory for now.
        </p>
      </div>
    </div>
  );
}
