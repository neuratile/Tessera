import type {
  OllamaStatus,
  ProviderConfigView,
  ProviderConnectionTestResult,
} from '@testing-ide/shared';
import { Check, Loader2, Trash2, X } from 'lucide-react';
import { useCallback, useEffect, useRef, useState } from 'react';

import { EmbeddingConfigPanel } from '@/components/settings/embedding-config-panel';
import { Button } from '@/components/ui/button';
import { Dialog } from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';
import { useDialogTitleId } from '@/lib/dialog-title';
import { getErrorMessage, ollama, providers } from '@/lib/ipc';
import { pickActiveProvider } from '@/lib/provider';
import { useAiStore } from '@/stores/ai-store';
import { useUiStore } from '@/stores/ui-store';

const PROVIDER_OPTIONS = [
  { id: 'ollama', label: 'Ollama (local)', requiresKey: false },
  { id: 'ollama-cloud', label: 'Ollama Cloud', requiresKey: true },
  { id: 'openai', label: 'OpenAI', requiresKey: true },
  { id: 'openrouter', label: 'OpenRouter', requiresKey: true },
  { id: 'anthropic', label: 'Anthropic', requiresKey: true },
  { id: 'gemini', label: 'Google Gemini', requiresKey: true },
] as const;

type ProviderOption = (typeof PROVIDER_OPTIONS)[number];

const DEFAULT_OLLAMA_BASE_URL = 'http://localhost:11434';

/**
 * Official endpoints the backend presets when no base URL is stored
 * (`providers/factory.rs` + `provider_connection_service.rs`). Shown as
 * read-only context only — cloud endpoints are not user-editable; the
 * Base URL field is reserved for Ollama (local).
 */
const PRESET_BASE_URLS: Record<Exclude<ProviderOption['id'], 'ollama'>, string> = {
  'ollama-cloud': 'https://ollama.com',
  openai: 'https://api.openai.com',
  openrouter: 'https://openrouter.ai/api',
  anthropic: 'https://api.anthropic.com',
  gemini: 'https://generativelanguage.googleapis.com',
};

const DEFAULT_MODELS: Record<ProviderOption['id'], string> = {
  'ollama': 'qwen2.5-coder:7b',
  'ollama-cloud': 'qwen2.5-coder:7b',
  'openai': 'gpt-4o',
  'openrouter': 'google/gemini-2.5-pro',
  'anthropic': 'claude-3-5-sonnet-latest',
  'gemini': 'gemini-1.5-flash',
};

/**
 * Resolve the base URL to display for a provider: Ollama (local) honours a
 * stored override; cloud providers are always pinned to the preset endpoint.
 */
function resolveBaseUrl(id: ProviderOption['id'], stored?: string | null): string {
  return id === 'ollama' ? (stored ?? DEFAULT_OLLAMA_BASE_URL) : PRESET_BASE_URLS[id];
}

/**
 * Settings sheet — provider config CRUD + live test.
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
  const [baseUrl, setBaseUrl] = useState(DEFAULT_OLLAMA_BASE_URL);
  const [model, setModel] = useState('qwen2.5-coder:7b');
  const [testResult, setTestResult] = useState<ProviderConnectionTestResult | null>(null);
  const [testing, setTesting] = useState(false);
  const [saving, setSaving] = useState(false);

  // Dynamic model options and server management state
  const [availableModels, setAvailableModels] = useState<string[]>([]);
  const [loadingModels, setLoadingModels] = useState(false);
  const [isCustomModel, setIsCustomModel] = useState(false);
  const [ollamaStatus, setOllamaStatus] = useState<OllamaStatus | null>(null);
  const [startingOllama, setStartingOllama] = useState(false);

  const ollamaIntervalRef = useRef<NodeJS.Timeout | null>(null);

  useEffect(() => {
    return () => {
      if (ollamaIntervalRef.current) {
        clearInterval(ollamaIntervalRef.current);
      }
    };
  }, []);

  const loadSavedConfig = useCallback((targetProvider: ProviderOption['id'], currentList: ProviderConfigView[]) => {
    const saved = currentList.find(c => c.provider === targetProvider);
    setBaseUrl(resolveBaseUrl(targetProvider, saved?.baseUrl));
    setModel(saved?.defaultModel ?? DEFAULT_MODELS[targetProvider]);
    setApiKey('');
    setTestResult(null);
    setIsCustomModel(false);
    setAvailableModels([]);
  }, []);

  const loadModels = useCallback((currentProvider: ProviderOption['id'], currentBaseUrl: string, currentApiKey: string) => {
    setLoadingModels(true);
    void (async () => {
      try {
        if (currentProvider === 'ollama') {
          const res = await providers.listOllamaModels(currentBaseUrl);
          setAvailableModels(res.map(m => m.name));
        } else {
          const res = await providers.testProviderConnection({
            provider: currentProvider,
            apiKey: currentApiKey.length > 0 ? currentApiKey : undefined,
            baseUrl: currentBaseUrl.length > 0 ? currentBaseUrl : undefined,
          });
          if (res.models && res.models.length > 0) {
            setAvailableModels(res.models);
          } else {
            setAvailableModels([]);
          }
        }
      } catch (err) {
        console.error('Failed to load models:', err);
        setAvailableModels([]);
      } finally {
        setLoadingModels(false);
      }
    })();
  }, []);

  const checkStatus = useCallback(() => {
    void (async () => {
      try {
        const status = await ollama.checkOllamaStatus();
        setOllamaStatus(status);
        if (status.running && provider === 'ollama') {
          setAvailableModels(status.models);
        }
      } catch (err) {
        console.error('Failed to check Ollama status:', err);
      }
    })();
  }, [provider]);

  const refresh = useCallback(() => {
    setLoading(true);
    setError(null);
    void (async () => {
      try {
        const next = await providers.listProviderConfigs();
        setList(next);
        const active = pickActiveProvider(next);
        setActiveProvider(active);
        if (active) {
          setProvider(active.provider);
          setBaseUrl(resolveBaseUrl(active.provider, active.baseUrl));
          setModel(active.defaultModel ?? DEFAULT_MODELS[active.provider]);
        }
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

  useEffect(() => {
    if (open) {
      if (provider === 'ollama') {
        checkStatus();
      } else {
        const configView = list.find((c) => c.provider === provider);
        const hasSavedKey = configView?.hasApiKey ?? false;
        if (hasSavedKey) {
          loadModels(provider, baseUrl, '');
        } else {
          setAvailableModels([]);
        }
      }
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, provider, list, loadModels, checkStatus]);

  const handleStartOllama = useCallback(() => {
    if (ollamaIntervalRef.current) {
      clearInterval(ollamaIntervalRef.current);
      ollamaIntervalRef.current = null;
    }
    setStartingOllama(true);
    setError(null);
    void (async () => {
      try {
        const resolvedUrl = await ollama.startOllamaServer();
        setBaseUrl(resolvedUrl);

        let attempts = 0;
        const interval = setInterval(() => {
          attempts++;
          void (async () => {
            try {
              const status = await ollama.checkOllamaStatus();
              setOllamaStatus(status);
              if (status.running) {
                clearInterval(interval);
                if (ollamaIntervalRef.current === interval) {
                  ollamaIntervalRef.current = null;
                }
                setStartingOllama(false);
                setAvailableModels(status.models);
                if (status.models.length > 0 && status.models[0]) {
                  setModel(status.models[0]);
                }
                return;
              }
            } catch (err) {
              console.error('Polling Ollama status error:', err);
            }

            if (attempts >= 10) {
              clearInterval(interval);
              if (ollamaIntervalRef.current === interval) {
                ollamaIntervalRef.current = null;
              }
              setStartingOllama(false);
              setError("Ollama started, but failed to connect within timeout.");
            }
          })();
        }, 1000);
        ollamaIntervalRef.current = interval;
      } catch (err) {
        setStartingOllama(false);
        setError(getErrorMessage(err));
      }
    })();
  }, []);

  // Only Ollama (local) exposes an editable base URL. Cloud providers
  // always use the official preset endpoint: sending '' explicitly
  // clears any previously stored base URL so the backend default
  // (`providers/factory.rs`) applies.
  const isLocalOllama = provider === 'ollama';
  const effectiveBaseUrl = isLocalOllama
    ? baseUrl.length > 0
      ? baseUrl
      : undefined
    : '';

  const handleSave = useCallback(() => {
    setSaving(true);
    setError(null);
    setTestResult(null);
    void (async () => {
      try {
        await providers.saveProviderConfig({
          provider,
          apiKey: apiKey.length > 0 ? apiKey : undefined,
          baseUrl: effectiveBaseUrl,
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
  }, [provider, apiKey, effectiveBaseUrl, model, refresh]);

  const handleTest = useCallback(() => {
    setTesting(true);
    setTestResult(null);
    void (async () => {
      try {
        const result = await providers.testProviderConnection({
          provider,
          apiKey: apiKey.length > 0 ? apiKey : undefined,
          baseUrl: effectiveBaseUrl,
        });
        setTestResult(result);
        if (result.models && result.models.length > 0) {
          setAvailableModels(result.models);
        }
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
  }, [provider, apiKey, effectiveBaseUrl]);

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
                      loadSavedConfig(p.id, list);
                    }}
                    className="sr-only"
                  />
                  <span>{p.label}</span>
                </label>
              ))}
            </div>

            {isLocalOllama ? (
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
                  onBlur={() => {
                    loadModels(provider, baseUrl, apiKey);
                  }}
                  placeholder={DEFAULT_OLLAMA_BASE_URL}
                  autoComplete="off"
                  spellCheck={false}
                />

                <div className="flex flex-col gap-1.5 mt-1 bg-muted/10 border border-border/60 p-2 rounded-md">
                  {ollamaStatus?.installed === false ? (
                    <p className="text-destructive text-[10px]">
                      Ollama CLI not found in PATH. Please install Ollama first.
                    </p>
                  ) : ollamaStatus?.running ? (
                    <p className="text-success text-[10px] flex items-center gap-1.5">
                      <span className="size-1.5 rounded-full bg-success animate-pulse" />
                      Ollama local server is running.
                    </p>
                  ) : (
                    <div className="flex items-center justify-between gap-2">
                      <p className="text-warning text-[10px] flex-1">
                        Ollama local server is not running.
                      </p>
                      <Button
                        type="button"
                        className="h-6 px-2 text-[10px] font-semibold"
                        onClick={handleStartOllama}
                        disabled={startingOllama}
                      >
                        {startingOllama ? (
                          <>
                            <Loader2 className="size-3 mr-1 animate-spin" />
                            Starting…
                          </>
                        ) : (
                          "Start Server"
                        )}
                      </Button>
                    </div>
                  )}
                </div>
              </div>
            ) : (
              <p className="text-muted-foreground text-[10px]">
                Endpoint preset to{' '}
                <span className="font-mono">{PRESET_BASE_URLS[provider]}</span>. Only an
                API key is needed.
              </p>
            )}

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
                  onBlur={() => {
                    if (apiKey.length > 0) {
                      loadModels(provider, baseUrl, apiKey);
                    }
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
              <div className="flex items-center justify-between">
                <label htmlFor="provider-model" className="text-xs font-medium">
                  Model
                </label>
                {availableModels.length > 0 && (
                  <button
                    type="button"
                    onClick={() => setIsCustomModel(!isCustomModel)}
                    className="text-primary hover:underline text-[10px] font-medium"
                  >
                    {isCustomModel ? 'Use dropdown' : 'Type custom...'}
                  </button>
                )}
              </div>
              {loadingModels ? (
                <div className="flex items-center gap-2 text-muted-foreground text-xs h-8 px-2.5 border border-input rounded-md bg-background/50">
                  <Loader2 className="size-3 animate-spin text-primary" />
                  <span>Loading models…</span>
                </div>
              ) : availableModels.length > 0 && !isCustomModel ? (
                <select
                  id="provider-model"
                  value={model}
                  onChange={(e) => {
                    const val = e.target.value;
                    if (val === '__custom__') {
                      setIsCustomModel(true);
                    } else {
                      setModel(val);
                    }
                  }}
                  className="border-input bg-background text-foreground focus:ring-primary/40 focus:border-primary flex h-8 w-full rounded-md border px-2 py-1 text-xs transition-colors focus:outline-none focus:ring-2 disabled:cursor-not-allowed disabled:opacity-50"
                >
                  {!availableModels.includes(model) && model && (
                    <option value={model}>{model} (current)</option>
                  )}
                  {availableModels.map((m) => (
                    <option key={m} value={m}>
                      {m}
                    </option>
                  ))}
                  <option value="__custom__">+ Type custom model...</option>
                </select>
              ) : (
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
              )}
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

          <EmbeddingConfigPanel />
        </div>
    </Dialog>
  );
}

