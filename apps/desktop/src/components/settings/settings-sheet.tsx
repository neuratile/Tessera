import type {
  ProviderConfigView,
  ProviderConnectionTestResult,
} from '@testing-ide/shared';
import { Check, Loader2, Trash2, X } from 'lucide-react';
import { useCallback, useEffect, useState } from 'react';

import { Button } from '@/components/ui/button';
import { Dialog } from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';
import { useDialogTitleId } from '@/lib/dialog-title';
import { getErrorMessage, providers } from '@/lib/ipc';
import { pickActiveProvider } from '@/lib/provider';
import { useAiStore } from '@/stores/ai-store';
import { useUiStore } from '@/stores/ui-store';

const PROVIDER_OPTIONS = [
  { id: 'ollama', label: 'Ollama (local)', requiresKey: false },
  { id: 'ollama-cloud', label: 'Ollama Cloud', requiresKey: true },
  { id: 'openai', label: 'OpenAI', requiresKey: true },
  { id: 'openrouter', label: 'OpenRouter', requiresKey: true },
  { id: 'anthropic', label: 'Anthropic', requiresKey: true },
] as const;

type ProviderOption = (typeof PROVIDER_OPTIONS)[number];

/**
 * Settings sheet — provider config CRUD + live test.
 *
 * Security:
 * - The plaintext API key is held in renderer state only for the
 *   duration of the form session. On Save the value travels through
 *   `save_provider_config` which encrypts it at rest with AES-GCM
 *   (Phase 6 `utils/crypto.rs`). The backend never returns it again;
 *   the listed `ProviderConfigView` carries `hasApiKey: boolean`.
 * - The "Test Connection" path uses `test_provider_connection` which
 *   does not persist the key. Live probes are only made for Ollama
 *   (local). Cloud providers return "credentials accepted" without
 *   echoing the key in any error path.
 */
export function SettingsSheet() {
  const open = useUiStore((s) => s.settingsOpen);
  const setOpen = useUiStore((s) => s.setSettingsOpen);
  const sandboxOptIn = useUiStore((s) => s.sandboxOptIn);
  const setSandboxOptIn = useUiStore((s) => s.setSandboxOptIn);
  const setActiveProvider = useAiStore((s) => s.setActiveProvider);

  const [list, setList] = useState<ProviderConfigView[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const [provider, setProvider] = useState<ProviderOption['id']>('ollama');
  const [apiKey, setApiKey] = useState('');
  const [baseUrl, setBaseUrl] = useState('http://localhost:11434');
  const [model, setModel] = useState('qwen2.5-coder:7b');
  const [testResult, setTestResult] = useState<ProviderConnectionTestResult | null>(null);
  const [testing, setTesting] = useState(false);
  const [saving, setSaving] = useState(false);

  const refresh = useCallback(() => {
    setLoading(true);
    setError(null);
    void (async () => {
      try {
        const next = await providers.listProviderConfigs();
        setList(next);
        setActiveProvider(pickActiveProvider(next));
      } catch (err) {
        setError(getErrorMessage(err));
      } finally {
        setLoading(false);
      }
    })();
  }, [setActiveProvider]);

  useEffect(() => {
    if (open) refresh();
  }, [open, refresh]);

  const handleSave = useCallback(() => {
    setSaving(true);
    setError(null);
    setTestResult(null);
    void (async () => {
      try {
        await providers.saveProviderConfig({
          provider,
          apiKey: apiKey.length > 0 ? apiKey : undefined,
          baseUrl: baseUrl.length > 0 ? baseUrl : undefined,
          defaultModel: model.length > 0 ? model : undefined,
          isActive: true,
        });
        // Clear the plaintext key from renderer memory once saved.
        setApiKey('');
        refresh();
      } catch (err) {
        setError(getErrorMessage(err));
      } finally {
        setSaving(false);
      }
    })();
  }, [provider, apiKey, baseUrl, model, refresh]);

  const handleTest = useCallback(() => {
    setTesting(true);
    setTestResult(null);
    void (async () => {
      try {
        const result = await providers.testProviderConnection({
          provider,
          apiKey: apiKey.length > 0 ? apiKey : undefined,
          baseUrl: baseUrl.length > 0 ? baseUrl : undefined,
        });
        setTestResult(result);
      } catch (err) {
        setTestResult({
          ok: false,
          message: getErrorMessage(err),
          latencyMs: 0,
          models: [],
        });
      } finally {
        setTesting(false);
      }
    })();
  }, [provider, apiKey, baseUrl]);

  const handleDelete = useCallback(
    (id: string) => {
      void (async () => {
        try {
          await providers.deleteProviderConfig(id);
          refresh();
        } catch (err) {
          setError(getErrorMessage(err));
        }
      })();
    },
    [refresh],
  );

  const requiresKey =
    PROVIDER_OPTIONS.find((p) => p.id === provider)?.requiresKey ?? false;

  const titleId = useDialogTitleId();

  return (
    <Dialog open={open} onClose={() => setOpen(false)} labelledBy={titleId}>
      <header className="flex h-8 shrink-0 items-center justify-between border-b border-border bg-card px-4">
          <h2 id={titleId} className="flex items-center gap-2">
            <span className="font-brand text-primary text-sm">tessera</span>
            <span className="text-[11px] font-semibold uppercase tracking-[0.12em] text-foreground">
              Settings
            </span>
          </h2>
          <Button type="button" size="icon" variant="ghost" onClick={() => setOpen(false)} aria-label="Close">
            <X className="size-4" />
          </Button>
        </header>

        <div className="flex-1 overflow-y-auto p-4 space-y-6">
          <section className="space-y-2">
            <h3 className="text-[10px] font-semibold uppercase tracking-[0.12em] text-muted-foreground">
              Local Test Execution
            </h3>
            <label className="flex cursor-pointer items-start gap-3 rounded-md border border-border bg-background px-3 py-2.5 text-xs transition-colors hover:border-primary/40">
              <input
                type="checkbox"
                checked={sandboxOptIn}
                onChange={(e) => setSandboxOptIn(e.target.checked)}
                className="mt-0.5 size-3.5 accent-primary"
              />
              <span className="min-w-0 flex-1">
                <span className="font-medium text-foreground">
                  Run generated tests in a local Docker sandbox
                </span>
                <span className="text-muted-foreground mt-1 block text-[10px] leading-relaxed">
                  Off by default. When on, the <span className="font-mono">Run</span> button on a
                  Test Cases artifact executes its generated tests inside a hardened,
                  network-isolated container on this machine. Code never leaves your computer.
                  Requires Docker to be installed and running.
                </span>
              </span>
            </label>
          </section>

          <section className="space-y-2">
            <h3 className="text-[10px] font-semibold uppercase tracking-[0.12em] text-muted-foreground">
              Configured Providers
            </h3>
            {loading ? (
              <p className="text-muted-foreground text-xs">Loading…</p>
            ) : list.length === 0 ? (
              <p className="text-muted-foreground text-xs">None yet.</p>
            ) : (
              <ul className="space-y-1.5">
                {list.map((c) => (
                  <li
                    key={c.id}
                    className="group flex items-center justify-between rounded-md border border-border bg-background px-3 py-2 text-xs transition-colors hover:border-primary/40"
                  >
                    <div className="min-w-0 flex-1">
                      <div className="flex items-center gap-2">
                        <span
                          aria-hidden="true"
                          className={`size-1.5 rounded-full ${c.isActive ? 'bg-primary' : 'bg-surface-3'}`}
                        />
                        <p className="font-medium text-foreground">{c.provider}</p>
                      </div>
                      <p className="text-muted-foreground mt-1 truncate font-mono text-[10px]">
                        {c.defaultModel ?? '(no default model)'} · key{' '}
                        {c.hasApiKey ? 'set' : 'missing'} · {c.isActive ? 'active' : 'inactive'}
                      </p>
                    </div>
                    <Button
                      type="button"
                      size="icon"
                      variant="ghost"
                      aria-label={`Delete ${c.provider}`}
                      onClick={() => handleDelete(c.id)}
                    >
                      <Trash2 className="size-3.5" />
                    </Button>
                  </li>
                ))}
              </ul>
            )}
          </section>

          <section className="space-y-3">
            <h3 className="text-[10px] font-semibold uppercase tracking-[0.12em] text-muted-foreground">
              Add / Update Provider
            </h3>

            <div className="grid grid-cols-2 gap-2">
              {PROVIDER_OPTIONS.map((p) => (
                <label
                  key={p.id}
                  className={`flex cursor-pointer items-center gap-2 rounded-md border px-3 py-2 text-xs transition-colors ${
                    provider === p.id
                      ? 'border-primary bg-primary/8 text-primary'
                      : 'border-border bg-background hover:bg-muted/50 hover:border-primary/40'
                  }`}
                >
                  <input
                    type="radio"
                    name="provider"
                    value={p.id}
                    checked={provider === p.id}
                    onChange={() => {
                      setProvider(p.id);
                      setTestResult(null);
                    }}
                    className="sr-only"
                  />
                  <span>{p.label}</span>
                </label>
              ))}
            </div>

            <div className="space-y-1.5">
              <label htmlFor="provider-base-url" className="text-xs font-medium">
                Base URL
              </label>
              <Input
                id="provider-base-url"
                value={baseUrl}
                onChange={(e) => {
                  setBaseUrl(e.target.value);
                }}
                placeholder={provider === 'ollama' ? 'http://localhost:11434' : 'optional'}
                autoComplete="off"
                spellCheck={false}
              />
            </div>

            {requiresKey ? (
              <div className="space-y-1.5">
                <label htmlFor="provider-api-key" className="text-xs font-medium">
                  API key
                </label>
                <Input
                  id="provider-api-key"
                  type="password"
                  value={apiKey}
                  onChange={(e) => {
                    setApiKey(e.target.value);
                  }}
                  placeholder="sk-…"
                  autoComplete="off"
                  spellCheck={false}
                />
                <p className="text-muted-foreground text-[10px]">
                  Stored encrypted at rest (AES-GCM). Never sent to anywhere except this provider.
                </p>
              </div>
            ) : null}

            <div className="space-y-1.5">
              <label htmlFor="provider-model" className="text-xs font-medium">
                Default model
              </label>
              <Input
                id="provider-model"
                value={model}
                onChange={(e) => {
                  setModel(e.target.value);
                }}
                placeholder={provider === 'ollama' ? 'qwen2.5-coder:7b' : 'gpt-4o'}
                autoComplete="off"
                spellCheck={false}
              />
            </div>

            {testResult !== null ? (
              <div
                className={`flex items-start gap-2 rounded-md border p-2 text-xs ${
                  testResult.ok
                    ? 'border-success/30 bg-success/5 text-success'
                    : 'border-destructive/30 bg-destructive/5 text-destructive'
                }`}
                role="status"
              >
                {testResult.ok ? (
                  <Check className="mt-0.5 size-3.5 shrink-0" />
                ) : (
                  <X className="mt-0.5 size-3.5 shrink-0" />
                )}
                <span className="min-w-0 flex-1">
                  {testResult.message}
                  {testResult.ok ? (
                    <span className="text-muted-foreground ml-1">
                      · {testResult.latencyMs} ms
                    </span>
                  ) : null}
                </span>
              </div>
            ) : null}

            {error !== null ? (
              <p className="text-destructive text-xs" role="alert">
                {error}
              </p>
            ) : null}

            <div className="flex items-center gap-2">
              <Button type="button" onClick={handleSave} disabled={saving}>
                {saving ? <Loader2 className="size-3.5 animate-spin" /> : null}
                Save
              </Button>
              <Button
                type="button"
                variant="outline"
                onClick={handleTest}
                disabled={testing}
              >
                {testing ? <Loader2 className="size-3.5 animate-spin" /> : null}
                Test connection
              </Button>
            </div>
          </section>
        </div>
    </Dialog>
  );
}
