import { REQUIRED_MODELS } from '@testing-ide/shared';

import type { OllamaSetupState } from '@/lib/ollama-setup';

import { Button } from '@/components/ui/button';

type Props = {
  isChecking: boolean;
  error: string | null;
  recommendedModel: string;
  setupState: OllamaSetupState | null;
  onRetry: () => void;
  onSkip: () => void;
};

function formatModelList(models: readonly string[]): string {
  return models.join(', ');
}

export function OllamaSetupModal({
  isChecking,
  error,
  recommendedModel,
  setupState,
  onRetry,
  onSkip,
}: Props) {
  const bootstrapCommand = REQUIRED_MODELS.some((model) => model === recommendedModel)
    ? 'pnpm bootstrap:ollama'
    : `pnpm bootstrap:ollama ${recommendedModel}`;

  return (
    <div
      aria-modal="true"
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/45 p-6"
      role="dialog"
    >
      <div className="w-full max-w-lg space-y-4 rounded-lg border border-border bg-background p-6 shadow-xl">
        <div className="space-y-1">
          <h2 className="text-lg font-semibold tracking-tight">Set up local Ollama</h2>
          <p className="text-muted-foreground text-sm">
            Testing IDE defaults to a local Ollama workflow on first launch. We need the runtime
            running and the required local models available before that path is ready.
          </p>
        </div>

        <div className="space-y-2 rounded-md border border-border bg-muted/30 p-3 text-sm">
          <p>
            Recommended local model:{' '}
            <code className="rounded bg-muted px-1 py-0.5 text-xs">{recommendedModel}</code>
          </p>
          <p>
            Bootstrap command:{' '}
            <code className="rounded bg-muted px-1 py-0.5 text-xs">{bootstrapCommand}</code>
          </p>
        </div>

        {error ? (
          <p className="text-destructive text-sm" role="alert">
            {error}
          </p>
        ) : null}

        {isChecking ? (
          <p className="text-muted-foreground text-sm">Checking local Ollama setup...</p>
        ) : setupState ? (
          <div className="space-y-2 text-sm">
            <p>Installed: {setupState.installed ? 'yes' : 'no'}</p>
            <p>Running: {setupState.running ? 'yes' : 'no'}</p>
            <p>
              Missing models:{' '}
              {setupState.missingModels.length === 0
                ? 'none'
                : formatModelList(setupState.missingModels)}
            </p>
          </div>
        ) : null}

        <p className="text-muted-foreground text-sm">
          Run the bootstrap command in the repo root, then come back here and re-check the local
          runtime.
        </p>

        <div className="flex flex-wrap justify-end gap-2">
          <Button type="button" variant="ghost" onClick={onSkip}>
            Skip for now
          </Button>
          <Button type="button" variant="outline" onClick={onRetry}>
            Re-check Ollama
          </Button>
        </div>
      </div>
    </div>
  );
}
