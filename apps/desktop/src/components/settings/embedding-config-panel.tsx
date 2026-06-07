import type {
  EmbeddingConfigView,
  EmbeddingPreset,
  EmbeddingProviderId,
  TestEmbeddingResult,
} from '@testing-ide/shared';
import { Check, Loader2, X } from 'lucide-react';
import { useCallback, useEffect, useMemo, useState } from 'react';

import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { embeddings, getErrorMessage } from '@/lib/ipc';
import { useEmbeddingStore } from '@/stores/embedding-store';
import { toast } from '@/stores/toast-store';
import { useWorkspaceStore } from '@/stores/workspace-store';

const PROVIDER_OPTIONS: ReadonlyArray<{
  id: EmbeddingProviderId;
  label: string;
  requiresKey: boolean;
}> = [
  { id: 'ollama', label: 'Ollama (local)', requiresKey: false },
  { id: 'ollama-cloud', label: 'Ollama Cloud', requiresKey: true },
  { id: 'openai', label: 'OpenAI', requiresKey: true },
  { id: 'gemini', label: 'Google Gemini', requiresKey: true },
  { id: 'huggingface', label: 'Hugging Face', requiresKey: true },
];

const CUSTOM_MODEL_VALUE = '__custom__';

/**
 * Providers whose API key can fall back to the LLM-provider config of
 * the same name (`embedding_config_service::resolve_api_key`). Hugging
 * Face has no LLM-side row, so its key always lives here.
 */
const LLM_KEY_FALLBACK_PROVIDERS: ReadonlySet<EmbeddingProviderId> = new Set([
  'ollama-cloud',
  'openai',
  'gemini',
]);

/**
 * Embeddings section of the Settings sheet
 * (plan/EMBEDDING_PROVIDER_SELECT.md §7.1).
 *
 * Embedding choice is independent of the LLM provider: the model preset
 * list comes from `list_embedding_presets` (single source of truth in
 * Rust), the dimension auto-fills from the Test probe, and switching
 * provider/model marks existing project indexes stale until re-indexed.
 */
export function EmbeddingConfigPanel() {
  const project = useWorkspaceStore((s) => s.project);
  const refreshIndexStatus = useEmbeddingStore((s) => s.refreshIndexStatus);

  const [presets, setPresets] = useState<EmbeddingPreset[]>([]);
  const [saved, setSaved] = useState<EmbeddingConfigView | null>(null);

  const [provider, setProvider] = useState<EmbeddingProviderId>('ollama');
  const [model, setModel] = useState('nomic-embed-text');
  const [isCustomModel, setIsCustomModel] = useState(false);
  const [dimension, setDimension] = useState(768);
  const [apiKey, setApiKey] = useState('');
  const [baseUrl, setBaseUrl] = useState('');
  const [showAdvanced, setShowAdvanced] = useState(false);

  const [error, setError] = useState<string | null>(null);
  const [testResult, setTestResult] = useState<TestEmbeddingResult | null>(null);
  const [testError, setTestError] = useState<string | null>(null);
  const [testing, setTesting] = useState(false);
  const [saving, setSaving] = useState(false);

  const providerPresets = useMemo(
    () =>
      presets.filter(
        (p) => p.provider === (provider === 'ollama-cloud' ? 'ollama' : provider),
      ),
    [presets, provider],
  );

  const applyView = useCallback((view: EmbeddingConfigView) => {
    setProvider(view.provider);
    setModel(view.model);
    setDimension(view.dimension);
    setBaseUrl(view.baseUrl ?? '');
    setApiKey('');
  }, []);

  useEffect(() => {
    void (async () => {
      try {
        const [presetList, view] = await Promise.all([
          embeddings.listEmbeddingPresets(),
          embeddings.getEmbeddingConfig(),
        ]);
        setPresets(presetList);
        setSaved(view);
        applyView(view);
      } catch (err) {
        setError(getErrorMessage(err));
      }
    })();
  }, [applyView]);

  // Saved model not in the preset list (custom / TEI) → custom input.
  useEffect(() => {
    setIsCustomModel(
      providerPresets.length === 0 || !providerPresets.some((p) => p.model === model),
    );
    // Only re-evaluate when the preset list context changes; typing a
    // custom model must not flip the input back to the dropdown.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [providerPresets]);

  const handleProviderChange = useCallback(
    (next: EmbeddingProviderId) => {
      setProvider(next);
      setTestResult(null);
      setTestError(null);
      setApiKey('');
      setBaseUrl('');
      const lookup = next === 'ollama-cloud' ? 'ollama' : next;
      const fallback = presets.find((p) => p.provider === lookup && p.isDefault);
      if (saved !== null && saved.provider === next) {
        applyView(saved);
      } else if (fallback !== undefined) {
        setModel(fallback.model);
        setDimension(fallback.dimension);
        setIsCustomModel(false);
      } else {
        setModel('');
        setIsCustomModel(true);
      }
    },
    [applyView, presets, saved],
  );

  const handleModelSelect = useCallback(
    (value: string) => {
      if (value === CUSTOM_MODEL_VALUE) {
        setIsCustomModel(true);
        return;
      }
      setModel(value);
      const preset = providerPresets.find((p) => p.model === value);
      if (preset !== undefined) {
        setDimension(preset.dimension);
      }
      setTestResult(null);
    },
    [providerPresets],
  );

  const buildArgs = useCallback(
    () => ({
      provider,
      model: model.trim(),
      dimension,
      baseUrl: baseUrl.trim().length > 0 ? baseUrl.trim() : undefined,
      apiKey: apiKey.length > 0 ? apiKey : undefined,
    }),
    [provider, model, dimension, baseUrl, apiKey],
  );

  const handleTest = useCallback(() => {
    setTesting(true);
    setTestResult(null);
    setTestError(null);
    void (async () => {
      try {
        const result = await embeddings.testEmbeddingConnection(buildArgs());
        setTestResult(result);
        // The probe reports the model's native dimension — trust it
        // over the preset/user value so saves never persist a mismatch.
        setDimension(result.detectedDimension);
      } catch (err) {
        setTestError(getErrorMessage(err));
      } finally {
        setTesting(false);
      }
    })();
  }, [buildArgs]);

  const handleSave = useCallback(() => {
    setSaving(true);
    setError(null);
    void (async () => {
      try {
        const view = await embeddings.saveEmbeddingConfig(buildArgs());
        setSaved(view);
        setApiKey('');
        toast.ok(
          'Embedding settings saved. Existing project indexes may need a re-index.',
          { title: 'Embeddings' },
        );
        if (project !== null) {
          await refreshIndexStatus(project.id);
        }
      } catch (err) {
        setError(getErrorMessage(err));
      } finally {
        setSaving(false);
      }
    })();
  }, [buildArgs, project, refreshIndexStatus]);

  const requiresKey =
    PROVIDER_OPTIONS.find((p) => p.id === provider)?.requiresKey ?? false;
  const hasSavedKey = saved !== null && saved.provider === provider && saved.hasApiKey;
  const hasLlmKeyFallback = LLM_KEY_FALLBACK_PROVIDERS.has(provider);
  const isCloudProvider = provider !== 'ollama';

  return (
    <section className="space-y-3" data-testid="embedding-config-panel">
      <h3 className="text-[10px] font-semibold uppercase tracking-[0.12em] text-muted-foreground">
        Embeddings
      </h3>
      <p className="text-muted-foreground text-[10px] leading-relaxed">
        The embedding model indexes your code for retrieval. It is independent of the LLM
        provider above — switching it marks existing project indexes stale until you
        re-analyze.
      </p>

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
              name="embedding-provider"
              value={p.id}
              checked={provider === p.id}
              onChange={() => handleProviderChange(p.id)}
              className="sr-only"
            />
            <span>{p.label}</span>
          </label>
        ))}
      </div>

      {isCloudProvider ? (
        <p className="text-warning text-[10px] leading-relaxed">
          Code snippets from your project will be sent to this provider for embedding.
          Choose Ollama (local) to keep all code on this machine.
        </p>
      ) : null}

      <div className="space-y-1.5">
        <div className="flex items-center justify-between">
          <label htmlFor="embedding-model" className="text-xs font-medium">
            Model
          </label>
          {providerPresets.length > 0 ? (
            <button
              type="button"
              onClick={() => setIsCustomModel(!isCustomModel)}
              className="text-primary hover:underline text-[10px] font-medium"
            >
              {isCustomModel ? 'Use presets' : 'Type custom...'}
            </button>
          ) : null}
        </div>
        {providerPresets.length > 0 && !isCustomModel ? (
          <select
            id="embedding-model"
            value={model}
            onChange={(e) => handleModelSelect(e.target.value)}
            className="border-input bg-background text-foreground focus:ring-primary/40 focus:border-primary flex h-8 w-full rounded-md border px-2 py-1 text-xs transition-colors focus:outline-none focus:ring-2 disabled:cursor-not-allowed disabled:opacity-50"
          >
            {providerPresets.map((p) => (
              <option key={p.model} value={p.model}>
                {p.model} ({p.dimension}d)
              </option>
            ))}
            <option value={CUSTOM_MODEL_VALUE}>+ Type custom model...</option>
          </select>
        ) : (
          <Input
            id="embedding-model"
            value={model}
            onChange={(e) => {
              setModel(e.target.value);
              setTestResult(null);
            }}
            placeholder={provider === 'huggingface' ? 'org/model-name' : 'model id'}
            autoComplete="off"
            spellCheck={false}
          />
        )}
        <p className="text-muted-foreground text-[10px]">
          Dimension: <span className="font-mono">{dimension}</span>
          {isCustomModel
            ? ' — run Test connection to detect the model’s native dimension.'
            : ''}
        </p>
      </div>

      {requiresKey ? (
        <div className="space-y-1.5">
          <label htmlFor="embedding-api-key" className="text-xs font-medium">
            API key
          </label>
          <Input
            id="embedding-api-key"
            type="password"
            value={apiKey}
            onChange={(e) => setApiKey(e.target.value)}
            placeholder={
              hasSavedKey
                ? 'Leave blank to keep the saved key'
                : hasLlmKeyFallback
                  ? 'Leave blank to reuse the LLM key for this provider'
                  : provider === 'huggingface'
                    ? 'hf_…'
                    : 'API key'
            }
            autoComplete="off"
            spellCheck={false}
          />
          <p className="text-muted-foreground text-[10px]">
            Stored encrypted at rest (AES-GCM).
            {hasLlmKeyFallback
              ? ' Falls back to the matching LLM provider key when left blank.'
              : ''}
          </p>
        </div>
      ) : null}

      <button
        type="button"
        onClick={() => setShowAdvanced(!showAdvanced)}
        className="text-primary hover:underline text-[10px] font-medium"
      >
        {showAdvanced ? 'Hide advanced' : 'Advanced…'}
      </button>
      {showAdvanced ? (
        <div className="space-y-1.5">
          <label htmlFor="embedding-base-url" className="text-xs font-medium">
            Base URL
          </label>
          <Input
            id="embedding-base-url"
            value={baseUrl}
            onChange={(e) => setBaseUrl(e.target.value)}
            placeholder={
              provider === 'huggingface'
                ? 'https://router.huggingface.co/hf-inference (or a TEI host)'
                : 'Use the provider default'
            }
            autoComplete="off"
            spellCheck={false}
          />
        </div>
      ) : null}

      {testResult !== null ? (
        <div
          className="border-success/30 bg-success/5 text-success flex items-start gap-2 rounded-md border p-2 text-xs"
          role="status"
        >
          <Check className="mt-0.5 size-3.5 shrink-0" />
          <span>
            Embedded in {testResult.latencyMs} ms · detected dimension{' '}
            {testResult.detectedDimension}
          </span>
        </div>
      ) : null}
      {testError !== null ? (
        <div
          className="border-destructive/30 bg-destructive/5 text-destructive flex items-start gap-2 rounded-md border p-2 text-xs"
          role="status"
        >
          <X className="mt-0.5 size-3.5 shrink-0" />
          <span className="min-w-0 flex-1">{testError}</span>
        </div>
      ) : null}
      {error !== null ? (
        <p className="text-destructive text-xs" role="alert">
          {error}
        </p>
      ) : null}

      <div className="flex items-center gap-2">
        <Button
          type="button"
          onClick={handleSave}
          disabled={saving || model.trim().length === 0}
          data-testid="embedding-save"
        >
          {saving ? <Loader2 className="size-3.5 animate-spin" /> : null}
          Save
        </Button>
        <Button
          type="button"
          variant="outline"
          onClick={handleTest}
          disabled={testing || model.trim().length === 0}
          data-testid="embedding-test"
        >
          {testing ? <Loader2 className="size-3.5 animate-spin" /> : null}
          Test connection
        </Button>
      </div>
    </section>
  );
}
