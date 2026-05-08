import Editor, { loader } from '@monaco-editor/react';
import { Circle, X } from 'lucide-react';
import * as monaco from 'monaco-editor';
import { useCallback, useMemo } from 'react';

import { languageFromFilename } from '@/lib/monaco-language';
import { usePrefersDark } from '@/lib/theme';
import { useEditorStore } from '@/stores/editor-store';

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

  const dark = usePrefersDark();
  const activeTab = useMemo(
    () => tabs.find((t) => t.id === activeId) ?? null,
    [tabs, activeId],
  );

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
      <div className="bg-muted/30 flex shrink-0 overflow-x-auto border-b border-border">
        {tabs.map((tab) => {
          const isActive = tab.id === activeId;
          return (
            <button
              key={tab.id}
              type="button"
              onClick={() => setActive(tab.id)}
              className={`group flex min-w-fit max-w-[220px] cursor-pointer items-center gap-2 border-r border-border px-3 py-1.5 text-xs transition-colors ${
                isActive
                  ? 'border-t-primary bg-background text-foreground border-t-2'
                  : 'text-muted-foreground hover:bg-muted/50 border-t-2 border-t-transparent'
              }`}
              title={tab.relativePath}
            >
              <span className="truncate">{tab.name}</span>
              <span
                role="button"
                aria-label={`Close ${tab.name}`}
                onClick={(e) => {
                  e.stopPropagation();
                  closeTab(tab.id);
                }}
                className="text-muted-foreground hover:bg-muted hover:text-foreground rounded p-0.5"
              >
                {tab.dirty ? (
                  <Circle className="text-primary size-3 fill-current" />
                ) : (
                  <X className="size-3" />
                )}
              </span>
            </button>
          );
        })}
      </div>
    );
  }
}

function EmptyState() {
  // Stitch empty-state pattern — centred copy over the mosaic
  // watermark, monospace brand mark to reinforce the IDE feel.
  return (
    <div className="relative flex flex-1 flex-col items-center justify-center p-8 text-center">
      <div className="bg-mosaic" aria-hidden="true" />
      <div className="relative z-10">
        <span className="font-brand text-primary/70 text-2xl">tessera</span>
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
