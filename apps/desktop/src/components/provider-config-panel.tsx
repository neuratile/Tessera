import type {
  ConnectionTestInput,
  ConnectionTestResult,
  LlmProvider,
  ProviderConfigView,
  SaveProviderArgs,
} from '@testing-ide/shared';
import { ConnectionTestSchema, LlmProviderIdSchema, SaveProviderArgsSchema } from '@testing-ide/shared';
import { useCallback, useEffect, useMemo, useState } from 'react';
import type { ZodError } from 'zod';

import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { getErrorMessage, providers } from '@/lib/ipc';
import { cn } from '@/lib/utils';

type ProviderFormState = {
  provider: LlmProvider;
  apiKey: string;
  baseUrl: string;
  defaultModel: string;
  isActive: boolean;
  clearSavedKey: boolean;
};

const PROVIDER_OPTIONS: ReadonlyArray<{ value: LlmProvider; label: string }> = [
  { value: 'ollama', label: 'Ollama Local' },
  { value: 'ollama-cloud', label: 'Ollama Cloud' },
  { value: 'openai', label: 'OpenAI' },
  { value: 'openrouter', label: 'OpenRouter' },
  { value: 'anthropic', label: 'Anthropic' },
];

const EMPTY_FORM_STATE: ProviderFormState = {
  provider: 'ollama',
  apiKey: '',
  baseUrl: '',
  defaultModel: 'qwen2.5-coder:7b',
  isActive: true,
  clearSavedKey: false,
};

function formatZodError(error: ZodError): string {
  const flat = error.flatten();
  const parts: string[] = [];

  for (const [field, messages] of Object.entries(flat.fieldErrors)) {
    if (Array.isArray(messages) && messages.length > 0) {
      parts.push(`${field}: ${messages.join(', ')}`);
    }
  }

  if (parts.length > 0) {
    return parts.join('; ');
  }

  if (flat.formErrors.length > 0) {
    return flat.formErrors.join('; ');
  }

  return 'Invalid input';
}

function buildSaveArgs(form: ProviderFormState): SaveProviderArgs {
  const parsed = SaveProviderArgsSchema.safeParse({
    provider: form.provider,
    apiKey: form.clearSavedKey
      ? ''
      : form.apiKey.trim() === ''
        ? undefined
        : form.apiKey.trim(),
    baseUrl: form.baseUrl.trim(),
    defaultModel: form.defaultModel.trim(),
    isActive: form.isActive,
  });

  if (!parsed.success) {
    throw new Error(formatZodError(parsed.error));
  }

  return parsed.data;
}

function buildConnectionTestArgs(form: ProviderFormState): ConnectionTestInput {
  const parsed = ConnectionTestSchema.safeParse({
    provider: form.provider,
    apiKey: form.clearSavedKey
      ? ''
      : form.apiKey.trim() === ''
        ? undefined
        : form.apiKey.trim(),
    baseUrl: form.baseUrl.trim(),
    defaultModel: form.defaultModel.trim() === '' ? undefined : form.defaultModel.trim(),
  });

  if (!parsed.success) {
    throw new Error(formatZodError(parsed.error));
  }

  return parsed.data;
}

function toEditForm(config: ProviderConfigView): ProviderFormState {
  return {
    provider: config.provider,
    apiKey: '',
    baseUrl: config.baseUrl ?? '',
    defaultModel: config.defaultModel ?? '',
    isActive: config.isActive,
    clearSavedKey: false,
  };
}

function parseProviderValue(value: string): LlmProvider | null {
  const parsed = LlmProviderIdSchema.safeParse(value);
  return parsed.success ? parsed.data : null;
}

export function ProviderConfigPanel() {
  const [configs, setConfigs] = useState<ProviderConfigView[]>([]);
  const [form, setForm] = useState<ProviderFormState>(EMPTY_FORM_STATE);
  const [editingConfigId, setEditingConfigId] = useState<string | null>(null);
  const [editingHasApiKey, setEditingHasApiKey] = useState(false);
  const [listError, setListError] = useState<string | null>(null);
  const [saveMessage, setSaveMessage] = useState<string | null>(null);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [testError, setTestError] = useState<string | null>(null);
  const [connectionResult, setConnectionResult] = useState<ConnectionTestResult | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [isSaving, setIsSaving] = useState(false);
  const [isTesting, setIsTesting] = useState(false);
  const [deletingConfigId, setDeletingConfigId] = useState<string | null>(null);

  const loadConfigs = useCallback(() => {
    setIsLoading(true);
    setListError(null);

    void providers
      .listProviderConfigs()
      .then((rows) => {
        setConfigs(rows);
      })
      .catch((error: unknown) => {
        setListError(getErrorMessage(error));
      })
      .finally(() => {
        setIsLoading(false);
      });
  }, []);

  useEffect(() => {
    loadConfigs();
  }, [loadConfigs]);

  const resetForm = useCallback(() => {
    setForm(EMPTY_FORM_STATE);
    setEditingConfigId(null);
    setEditingHasApiKey(false);
    setSaveError(null);
    setSaveMessage(null);
    setTestError(null);
    setConnectionResult(null);
  }, []);

  const handleFieldChange = useCallback(
    <K extends keyof ProviderFormState>(field: K, value: ProviderFormState[K]) => {
      setForm((current) => ({
        ...current,
        [field]: value,
      }));
    },
    [],
  );

  const handleEditConfig = useCallback((config: ProviderConfigView) => {
    setForm(toEditForm(config));
    setEditingConfigId(config.id);
    setEditingHasApiKey(config.hasApiKey);
    setSaveError(null);
    setSaveMessage(null);
    setTestError(null);
    setConnectionResult(null);
  }, []);

  const handleSaveConfig = useCallback(() => {
    let args: SaveProviderArgs;
    try {
      args = buildSaveArgs(form);
    } catch (error) {
      setSaveError(error instanceof Error ? error.message : String(error));
      return;
    }

    setIsSaving(true);
    setSaveError(null);
    setSaveMessage(null);

    void providers
      .saveProviderConfig(args)
      .then((id) => {
        setSaveMessage(`Saved config for ${args.provider}.`);
        setEditingConfigId(id);
        setEditingHasApiKey(args.apiKey !== undefined ? args.apiKey.trim() !== '' : editingHasApiKey);
        handleFieldChange('apiKey', '');
        handleFieldChange('clearSavedKey', false);
        loadConfigs();
      })
      .catch((error: unknown) => {
        setSaveError(getErrorMessage(error));
      })
      .finally(() => {
        setIsSaving(false);
      });
  }, [editingHasApiKey, form, handleFieldChange, loadConfigs]);

  const handleDeleteConfig = useCallback(
    (config: ProviderConfigView) => {
      setDeletingConfigId(config.id);
      setListError(null);

      void providers
        .deleteProviderConfig(config.id)
        .then(() => {
          if (editingConfigId === config.id) {
            resetForm();
          }
          loadConfigs();
        })
        .catch((error: unknown) => {
          setListError(getErrorMessage(error));
        })
        .finally(() => {
          setDeletingConfigId(null);
        });
    },
    [editingConfigId, loadConfigs, resetForm],
  );

  const runConnectionTest = useCallback((args: ConnectionTestInput) => {
    setIsTesting(true);
    setTestError(null);
    setConnectionResult(null);

    void providers
      .testProviderConnection(args)
      .then((result) => {
        setConnectionResult(result);
      })
      .catch((error: unknown) => {
        setTestError(getErrorMessage(error));
      })
      .finally(() => {
        setIsTesting(false);
      });
  }, []);

  const handleTestCurrentForm = useCallback(() => {
    try {
      runConnectionTest(buildConnectionTestArgs(form));
    } catch (error) {
      setTestError(error instanceof Error ? error.message : String(error));
    }
  }, [form, runConnectionTest]);

  const handleTestSavedConfig = useCallback(
    (config: ProviderConfigView) => {
      const args: ConnectionTestInput = {
        provider: config.provider,
        baseUrl: config.baseUrl ?? '',
      };

      if (typeof config.defaultModel === 'string' && config.defaultModel.length > 0) {
        args.defaultModel = config.defaultModel;
      }

      runConnectionTest(args);
    },
    [runConnectionTest],
  );

  const visibleModels = useMemo(
    () => connectionResult?.models.slice(0, 12) ?? [],
    [connectionResult],
  );

  return (
    <section className="space-y-4 rounded-lg border border-border p-4">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="space-y-1">
          <h2 className="text-sm font-medium">LLM Provider Configs</h2>
          <p className="text-muted-foreground text-xs">
            Save encrypted provider settings locally, test them, and keep one config per provider.
          </p>
        </div>
        <div className="flex flex-wrap gap-2">
          <Button type="button" variant="outline" size="sm" onClick={loadConfigs} disabled={isLoading}>
            Refresh
          </Button>
          <Button type="button" variant="ghost" size="sm" onClick={resetForm}>
            New config
          </Button>
        </div>
      </div>

      <div className="grid gap-4 lg:grid-cols-[minmax(0,1.15fr)_minmax(0,0.85fr)]">
        <div className="space-y-3">
          <div className="grid gap-2">
            <label className="text-xs font-medium" htmlFor="provider-kind">
              Provider
            </label>
            <select
              id="provider-kind"
              className={cn(
                'border-input bg-background ring-offset-background focus-visible:ring-ring flex h-9 w-full rounded-md border px-3 py-1 text-sm shadow-sm transition-colors focus-visible:outline-none focus-visible:ring-1 disabled:cursor-not-allowed disabled:opacity-50',
              )}
              value={form.provider}
              onChange={(event) => {
                const provider = parseProviderValue(event.target.value);
                if (provider !== null) {
                  handleFieldChange('provider', provider);
                }
              }}
            >
              {PROVIDER_OPTIONS.map((option) => (
                <option key={option.value} value={option.value}>
                  {option.label}
                </option>
              ))}
            </select>
          </div>

          <div className="grid gap-2">
            <label className="text-xs font-medium" htmlFor="provider-api-key">
              API key
            </label>
            <Input
              id="provider-api-key"
              type="password"
              autoComplete="off"
              placeholder={editingHasApiKey ? 'Leave blank to keep the saved key' : 'Optional for Ollama Local'}
              value={form.apiKey}
              onChange={(event) => {
                handleFieldChange('apiKey', event.target.value);
              }}
            />
            {editingHasApiKey ? (
              <label className="text-muted-foreground flex items-center gap-2 text-xs">
                <input
                  checked={form.clearSavedKey}
                  type="checkbox"
                  onChange={(event) => {
                    handleFieldChange('clearSavedKey', event.target.checked);
                  }}
                />
                Clear the saved key on the next save
              </label>
            ) : null}
          </div>

          <div className="grid gap-2">
            <label className="text-xs font-medium" htmlFor="provider-base-url">
              Base URL
            </label>
            <Input
              id="provider-base-url"
              placeholder="Use the provider default"
              value={form.baseUrl}
              onChange={(event) => {
                handleFieldChange('baseUrl', event.target.value);
              }}
            />
          </div>

          <div className="grid gap-2">
            <label className="text-xs font-medium" htmlFor="provider-default-model">
              Default model
            </label>
            <Input
              id="provider-default-model"
              placeholder="Model id used by generate_artifact"
              value={form.defaultModel}
              onChange={(event) => {
                handleFieldChange('defaultModel', event.target.value);
              }}
            />
          </div>

          <label className="text-muted-foreground flex items-center gap-2 text-xs">
            <input
              checked={form.isActive}
              type="checkbox"
              onChange={(event) => {
                handleFieldChange('isActive', event.target.checked);
              }}
            />
            Enable this config for generation
          </label>

          {saveError ? (
            <p className="text-destructive text-sm" role="alert">
              {saveError}
            </p>
          ) : null}
          {saveMessage ? <p className="text-sm">{saveMessage}</p> : null}

          <div className="flex flex-wrap gap-2">
            <Button type="button" onClick={handleSaveConfig} disabled={isSaving}>
              {isSaving ? 'Saving...' : editingConfigId ? 'Update config' : 'Save config'}
            </Button>
            <Button type="button" variant="outline" onClick={handleTestCurrentForm} disabled={isTesting}>
              {isTesting ? 'Testing...' : 'Test connection'}
            </Button>
          </div>
        </div>

        <div className="space-y-3 rounded-md border border-border bg-muted/20 p-3">
          <div className="space-y-1">
            <h3 className="text-sm font-medium">Connection test</h3>
            <p className="text-muted-foreground text-xs">
              Runs a minimal provider probe and returns latency plus any model ids the provider exposes.
            </p>
          </div>

          {testError ? (
            <p className="text-destructive text-sm" role="alert">
              {testError}
            </p>
          ) : null}

          {connectionResult ? (
            <div className="space-y-2 text-sm">
              <p>
                Status:{' '}
                <span className={connectionResult.ok ? 'text-green-700' : 'text-destructive'}>
                  {connectionResult.ok ? 'ok' : 'error'}
                </span>
              </p>
              <p>Latency: {connectionResult.latencyMs} ms</p>
              <p>{connectionResult.message}</p>
              <p>Models returned: {connectionResult.models.length}</p>
              {visibleModels.length > 0 ? (
                <div className="flex flex-wrap gap-2 pt-1">
                  {visibleModels.map((model) => (
                    <code key={model} className="rounded bg-muted px-1 py-0.5 text-xs">
                      {model}
                    </code>
                  ))}
                </div>
              ) : null}
              {connectionResult.models.length > visibleModels.length ? (
                <p className="text-muted-foreground text-xs">
                  Showing the first {visibleModels.length} of {connectionResult.models.length} models.
                </p>
              ) : null}
            </div>
          ) : (
            <p className="text-muted-foreground text-sm">No provider test has been run yet.</p>
          )}
        </div>
      </div>

      <div className="space-y-3">
        <div className="flex items-center justify-between gap-3">
          <h3 className="text-sm font-medium">Saved configs</h3>
          <span className="text-muted-foreground text-xs">{configs.length} total</span>
        </div>

        {listError ? (
          <p className="text-destructive text-sm" role="alert">
            {listError}
          </p>
        ) : null}

        {isLoading ? (
          <p className="text-muted-foreground text-sm">Loading saved configs...</p>
        ) : configs.length === 0 ? (
          <p className="text-muted-foreground text-sm">No provider configs saved yet.</p>
        ) : (
          <div className="space-y-2">
            {configs.map((config) => (
              <div
                key={config.id}
                className="flex flex-col gap-3 rounded-md border border-border p-3 lg:flex-row lg:items-center lg:justify-between"
              >
                <div className="space-y-1 text-sm">
                  <p className="font-medium">{config.provider}</p>
                  <p className="text-muted-foreground text-xs">
                    Model: {config.defaultModel ?? 'not set'} | Base URL: {config.baseUrl ?? 'default'}
                  </p>
                  <p className="text-muted-foreground text-xs">
                    Key: {config.hasApiKey ? 'saved' : 'missing'} | Active: {config.isActive ? 'yes' : 'no'}
                  </p>
                </div>
                <div className="flex flex-wrap gap-2">
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    onClick={() => {
                      handleEditConfig(config);
                    }}
                  >
                    Edit
                  </Button>
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    onClick={() => {
                      handleTestSavedConfig(config);
                    }}
                    disabled={isTesting}
                  >
                    Test
                  </Button>
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    onClick={() => {
                      handleDeleteConfig(config);
                    }}
                    disabled={deletingConfigId === config.id}
                  >
                    {deletingConfigId === config.id ? 'Deleting...' : 'Delete'}
                  </Button>
                </div>
              </div>
            ))}
          </div>
        )}
      </div>
    </section>
  );
}
