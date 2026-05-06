import type {
  ProviderConfigView,
  ProviderConnectionTestResult,
} from '@testing-ide/shared';
import { Check, Loader2, Trash2, X } from 'lucide-react';
import { useCallback, useEffect, useState } from 'react';

import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { IpcError, providers } from '@/lib/ipc';
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
        const active = next.find((c) => c.isActive) ?? next[0] ?? null;
        setActiveProvider(active);
      } catch (err) {
        setError(err instanceof IpcError ? err.message : String(err));
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
        setError(err instanceof IpcError ? err.message : String(err));
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
          message: err instanceof IpcError ? err.message : String(err),
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
          setError(err instanceof IpcError ? err.message : String(err));
        }
      })();
    },
    [refresh],
  );

  const requiresKey =
    PROVIDER_OPTIONS.find((p) => p.id === provider)?.requiresKey ?? false;

  if (!open) return null;

  return (
    <>
      <div
        className="bg-background/80 fixed inset-0 z-40 backdrop-blur-sm"
        onClick={() => setOpen(false)}
        aria-hidden="true"
      />
      <aside
        className="fixed inset-y-0 right-0 z-50 flex w-full max-w-md flex-col border-l border-border bg-background shadow-2xl"
        role="dialog"
        aria-label="Settings"
      >
        <header className="flex items-center justify-between border-b border-border px-4 py-3">
          <h2 className="text-base font-semibold tracking-tight">Settings</h2>
          <Button type="button" size="icon" variant="ghost" onClick={() => setOpen(false)} aria-label="Close">
            <X className="size-4" />
          </Button>
        </header>

        <div className="flex-1 overflow-y-auto p-4 space-y-6">
          <section className="space-y-3">
            <h3 className="text-sm font-medium">Configured providers</h3>
            {loading ? (
              <p className="text-muted-foreground text-xs">Loading…</p>
            ) : list.length === 0 ? (
              <p className="text-muted-foreground text-xs">None yet.</p>
            ) : (
              <ul className="space-y-1">
                {list.map((c) => (
                  <li
                    key={c.id}
                    className="flex items-center justify-between rounded-md border border-border bg-card px-3 py-2 text-xs"
                  >
                    <div className="min-w-0">
                      <p className="font-medium">{c.provider}</p>
                      <p className="text-muted-foreground truncate text-[10px]">
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
            <h3 className="text-sm font-medium">Add / update provider</h3>

            <div className="grid grid-cols-2 gap-2">
              {PROVIDER_OPTIONS.map((p) => (
                <label
                  key={p.id}
                  className={`flex cursor-pointer items-center gap-2 rounded-md border px-3 py-2 text-xs transition-colors ${
                    provider === p.id
                      ? 'border-primary bg-primary/5 text-primary'
                      : 'border-border bg-card hover:bg-muted/50'
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
              <p
                className={`flex items-center gap-1.5 text-xs ${
                  testResult.ok ? 'text-green-600 dark:text-green-400' : 'text-destructive'
                }`}
                role="status"
              >
                {testResult.ok ? (
                  <Check className="size-3.5" />
                ) : (
                  <X className="size-3.5" />
                )}
                {testResult.message}
                {testResult.ok ? (
                  <span className="text-muted-foreground">({testResult.latencyMs} ms)</span>
                ) : null}
              </p>
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
      </aside>
    </>
  );
}
