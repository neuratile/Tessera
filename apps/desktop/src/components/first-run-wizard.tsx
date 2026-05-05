import type { HealthStatus } from '@testing-ide/shared';
import { useCallback, useEffect, useState } from 'react';

import { Button } from '@/components/ui/button';
import { recommendTier } from '@/lib/hardware-tier';
import { health, IpcError } from '@/lib/ipc';
import { markOnboardingComplete } from '@/lib/onboarding';

type Props = {
  /** Called once the user dismisses the wizard. Parent should re-render. */
  onComplete: () => void;
};

/**
 * One-screen onboarding flow shown the first time the desktop app launches.
 *
 * Calls `health_check` to detect OS / RAM / CPU, displays the result, and
 * recommends a local Ollama model tier based on total memory. Persists the
 * "seen" flag in `localStorage` so the wizard does not reappear on the next
 * launch. Settings UI (later phase) will let users re-trigger it.
 */
export function FirstRunWizard({ onComplete }: Props) {
  const [status, setStatus] = useState<HealthStatus | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    void health
      .healthCheck()
      .then((s) => {
        if (!cancelled) setStatus(s);
      })
      .catch((err: unknown) => {
        if (cancelled) return;
        setError(err instanceof IpcError ? err.message : String(err));
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const handleFinish = useCallback(() => {
    markOnboardingComplete();
    onComplete();
  }, [onComplete]);

  const tier = status ? recommendTier(status) : null;

  return (
    <div className="mx-auto flex min-h-screen max-w-2xl flex-col gap-6 p-8">
      <header className="space-y-1">
        <h1 className="text-2xl font-semibold tracking-tight">Welcome to Testing IDE</h1>
        <p className="text-muted-foreground text-sm">
          Local-first, AI-assisted test artifact generation. We&rsquo;ll detect your hardware and
          recommend a model that fits.
        </p>
      </header>

      <section className="space-y-3 rounded-lg border border-border p-4">
        <h2 className="text-sm font-medium">System detection</h2>
        {error ? (
          <p className="text-destructive text-sm" role="alert">
            {error}
          </p>
        ) : status === null ? (
          <p className="text-muted-foreground text-sm">Probing local hardware…</p>
        ) : (
          <dl className="grid grid-cols-[max-content_1fr] gap-x-4 gap-y-1 text-sm">
            <dt className="text-muted-foreground">OS</dt>
            <dd>
              {status.osName} {status.osVersion}
            </dd>
            <dt className="text-muted-foreground">CPUs</dt>
            <dd>{status.cpuCount}</dd>
            <dt className="text-muted-foreground">Memory</dt>
            <dd>
              {(status.totalMemoryMb / 1024).toFixed(1)} GB total ·{' '}
              {(status.availableMemoryMb / 1024).toFixed(1)} GB available
            </dd>
            <dt className="text-muted-foreground">Database</dt>
            <dd>{status.dbOk ? 'reachable' : 'unreachable'}</dd>
          </dl>
        )}
      </section>

      {tier ? (
        <section className="space-y-2 rounded-lg border border-border p-4">
          <h2 className="text-sm font-medium">Recommended model</h2>
          <p className="text-sm">
            <code className="rounded bg-muted px-1 py-0.5 text-xs">{tier.recommendedModel}</code>
            <span className="text-muted-foreground"> — {tier.label}</span>
          </p>
          <p className="text-muted-foreground text-xs">{tier.rationale}</p>
        </section>
      ) : null}

      <footer className="flex justify-end">
        <Button type="button" onClick={handleFinish} disabled={status === null && error === null}>
          Get started
        </Button>
      </footer>
    </div>
  );
}
