import type { ProviderConfigView } from '@testing-ide/shared';
import { Check, ChevronUp, Cpu, Loader2 } from 'lucide-react';
import { useCallback, useEffect, useRef, useState } from 'react';

import { getErrorMessage, providers as providersIpc } from '@/lib/ipc';
import { useAiStore } from '@/stores/ai-store';
import { toast } from '@/stores/toast-store';
import { useUiStore } from '@/stores/ui-store';
import { useWorkspaceStore } from '@/stores/workspace-store';

/**
 * Bottom status bar. Surfaces project status, analysis pipeline
 * progress, the active provider (with a switcher popover), the
 * selected file, and the latest tree-load / analysis error.
 */
export function StatusBar() {
  const project = useWorkspaceStore((s) => s.project);
  const selectedPath = useWorkspaceStore((s) => s.selectedPath);
  const treeError = useWorkspaceStore((s) => s.treeError);
  const analysis = useWorkspaceStore((s) => s.analysis);

  return (
    <footer className="flex h-7 shrink-0 items-center justify-between gap-2 border-t border-border bg-surface-3 px-3 font-mono text-[11px] text-muted-foreground">
      <div className="flex items-center gap-4">
        {project ? (
          <>
            <span className="flex items-center gap-1" data-testid="project-status">
              <span className="size-1.5 rounded-full bg-primary" aria-hidden="true" />
              {project.status}
            </span>
            <span>{project.fileCount} files</span>
          </>
        ) : (
          <span>no project</span>
        )}
        {analysis.status === 'pending' ? (
          <span
            className="flex items-center gap-1 text-muted-foreground"
            data-testid="analysis-status"
          >
            <Loader2 className="size-3 animate-spin" />
            analysing…
          </span>
        ) : analysis.status === 'ready' ? (
          <span className="text-muted-foreground" data-testid="analysis-status">
            {analysis.outcome.chunksEmbedded} chunks · {analysis.outcome.filesParsed} parsed
          </span>
        ) : analysis.status === 'error' ? (
          <span
            className="text-destructive truncate"
            role="alert"
            title={analysis.message}
            data-testid="analysis-status"
          >
            analysis failed
          </span>
        ) : null}
      </div>
      <div className="flex items-center gap-3">
        {treeError !== null ? (
          <span className="text-destructive truncate" role="alert" title={treeError}>
            {treeError}
          </span>
        ) : null}
        {selectedPath !== null ? (
          <code className="text-muted-foreground truncate">{selectedPath}</code>
        ) : null}
        <ProviderSwitcher />
      </div>
    </footer>
  );
}

/**
 * Status-bar provider switcher.
 *
 * Replaces the previous static "Provider: …" line in the Stitch
 * mock with a clickable popover that lists every configured
 * provider and lets the user flip the active row without opening
 * the Settings sheet. Saving with `isActive: true` and no `apiKey`
 * is safe — `provider_config_service::save_config` preserves the
 * stored encrypted blob when `apiKey` is omitted (covered by the
 * `save_config_preserves_existing_key_when_api_key_omitted` Rust
 * test).
 */
function ProviderSwitcher() {
  const providers = useAiStore((s) => s.providers);
  const activeProvider = useAiStore((s) => s.activeProvider);
  const setProviders = useAiStore((s) => s.setProviders);
  const setActiveProvider = useAiStore((s) => s.setActiveProvider);
  const setSettingsOpen = useUiStore((s) => s.setSettingsOpen);

  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const containerRef = useRef<HTMLDivElement | null>(null);

  // Close on outside click + Escape so the popover behaves like the
  // recent-projects one in the toolbar.
  useEffect(() => {
    if (!open) return;
    const clickHandler = (event: MouseEvent) => {
      const node = containerRef.current;
      if (node !== null && event.target instanceof Node && !node.contains(event.target)) {
        setOpen(false);
      }
    };
    const keyHandler = (event: KeyboardEvent) => {
      if (event.key === 'Escape') setOpen(false);
    };
    window.addEventListener('mousedown', clickHandler);
    window.addEventListener('keydown', keyHandler);
    return () => {
      window.removeEventListener('mousedown', clickHandler);
      window.removeEventListener('keydown', keyHandler);
    };
  }, [open]);

  const handleSwitch = useCallback(
    (row: ProviderConfigView) => {
      if (row.isActive) {
        setOpen(false);
        return;
      }
      setBusy(row.id);
      void (async () => {
        try {
          await providersIpc.saveProviderConfig({
            provider: row.provider,
            isActive: true,
          });
          const next = await providersIpc.listProviderConfigs();
          setProviders(next);
          const active = next.find((c) => c.isActive) ?? null;
          setActiveProvider(active);
          toast.ok(`Provider set to ${row.provider}`, { title: 'Provider' });
          setOpen(false);
        } catch (err) {
          toast.err(getErrorMessage(err), {
            title: 'Provider switch failed',
          });
        } finally {
          setBusy(null);
        }
      })();
    },
    [setActiveProvider, setProviders],
  );

  if (providers.length === 0 && activeProvider === null) {
    return (
      <button
        type="button"
        onClick={() => setSettingsOpen(true)}
        className="text-muted-foreground hover:text-foreground flex items-center gap-1 text-[11px] transition-colors"
        title="Open settings to configure a provider"
      >
        <Cpu className="size-3" /> no provider
      </button>
    );
  }

  const display = activeProvider ?? providers.find((p) => p.isActive) ?? null;

  return (
    <div ref={containerRef} className="relative">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        aria-haspopup="listbox"
        aria-expanded={open}
        className="text-muted-foreground hover:text-foreground hover:bg-surface-2 flex items-center gap-1 rounded px-1.5 py-0.5 text-[11px] transition-colors"
        title="Switch active connection"
      >
        <Cpu className="size-3" />
        <span className="text-foreground font-medium">{display?.provider ?? 'Select connection'}</span>
        {display?.defaultModel !== undefined && display?.defaultModel !== null ? (
          <span>· {display.defaultModel}</span>
        ) : null}
        <ChevronUp className={`size-3 transition-transform ${open ? '' : 'rotate-180'}`} />
      </button>
      {open ? (
        <ul
          role="listbox"
          aria-label="Active connection"
          className="bg-card border-border absolute bottom-full right-0 mb-1 max-h-72 w-64 overflow-y-auto rounded-md border shadow-lg"
        >
          {providers.length === 0 ? (
            <li className="text-muted-foreground p-3 text-xs">
              No providers configured. Open Settings to add one.
            </li>
          ) : (
            providers.map((p) => {
              const isBusy = busy === p.id;
              return (
                <li key={p.id}>
                  <button
                    type="button"
                    role="option"
                    aria-selected={p.isActive}
                    onClick={() => handleSwitch(p)}
                    disabled={isBusy}
                    className="hover:bg-muted/40 disabled:opacity-60 flex w-full items-center justify-between gap-2 px-3 py-2 text-left text-xs transition-colors"
                  >
                    <span className="min-w-0 flex-1">
                      <span className="text-foreground block font-medium">{p.provider}</span>
                      <span className="text-muted-foreground block truncate font-mono text-[10px]">
                        {p.defaultModel ?? '(no default model)'}
                      </span>
                    </span>
                    {isBusy ? (
                      <Loader2 className="text-muted-foreground size-3.5 shrink-0 animate-spin" />
                    ) : p.isActive ? (
                      <Check className="text-primary size-3.5 shrink-0" />
                    ) : null}
                  </button>
                </li>
              );
            })
          )}
          <li className="border-border border-t">
            <button
              type="button"
              onClick={() => {
                setOpen(false);
                setSettingsOpen(true);
              }}
              className="hover:bg-muted/40 text-primary w-full px-3 py-2 text-left text-xs transition-colors"
            >
              Manage providers in Settings…
            </button>
          </li>
        </ul>
      ) : null}
    </div>
  );
}
