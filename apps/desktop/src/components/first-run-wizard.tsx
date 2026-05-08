import type { HealthStatus, ProviderConnectionTestResult } from '@testing-ide/shared';
import { ArrowRight, Check, Cpu, HardDrive, Loader2, Server, X } from 'lucide-react';
import type { ReactNode } from 'react';
import { useCallback, useEffect, useState } from 'react';

import { Button } from '@/components/ui/button';
import { recommendTier, type HardwareTier } from '@/lib/hardware-tier';
import { health, IpcError, providers } from '@/lib/ipc';
import { markOnboardingComplete } from '@/lib/onboarding';

type Props = {
  /** Called once the user dismisses the wizard. Parent should re-render. */
  onComplete: () => void;
};

type Step = 1 | 2 | 3 | 4;

/**
 * Four-step onboarding flow shown the first time the desktop app launches.
 *
 * 1. Welcome — value prop.
 * 2. Hardware — calls real `health_check`, recommends a local model
 *    tier from `lib/hardware-tier.ts`. No mocked CPU/RAM.
 * 3. Local engine — pings the Ollama daemon via the same IPC
 *    `test_provider_connection` used by the Settings sheet, so the
 *    user finds out at onboarding time whether `ollama serve` is up.
 * 4. Pick a model — saves a default Ollama provider config so the AI
 *    panel works immediately on first launch.
 */
export function FirstRunWizard({ onComplete }: Props) {
  const [step, setStep] = useState<Step>(1);
  const [status, setStatus] = useState<HealthStatus | null>(null);
  const [healthError, setHealthError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    void health
      .healthCheck()
      .then((s) => {
        if (!cancelled) setStatus(s);
      })
      .catch((err: unknown) => {
        if (cancelled) return;
        setHealthError(err instanceof IpcError ? err.message : String(err));
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const tier = status === null ? null : recommendTier(status);

  const finish = useCallback(() => {
    markOnboardingComplete();
    onComplete();
  }, [onComplete]);

  return (
    <div className="bg-background flex h-screen w-screen items-center justify-center p-4">
      <div className="bg-card flex h-[540px] w-full max-w-2xl flex-col overflow-hidden rounded-2xl border border-border shadow-2xl">
        <Header step={step} />
        <div className="flex-1 overflow-y-auto p-8">
          {step === 1 && <StepOne />}
          {step === 2 && <StepTwo status={status} error={healthError} tier={tier} />}
          {step === 3 && <StepThree />}
          {step === 4 && <StepFour tier={tier} />}
        </div>
        <Footer step={step} setStep={setStep} finish={finish} />
      </div>
    </div>
  );
}

function Header({ step }: { step: Step }) {
  return (
    <div className="bg-muted/20 shrink-0 border-b border-border p-6">
      <h1 className="text-primary mb-3 flex items-center gap-2 text-base font-bold tracking-tight">
        <Server className="size-5" />
        Testing IDE setup
      </h1>
      <div className="flex gap-1.5">
        {[1, 2, 3, 4].map((s) => (
          <div
            key={s}
            className={`h-1.5 flex-1 rounded-full transition-colors ${
              step >= s ? 'bg-primary' : 'bg-muted'
            }`}
          />
        ))}
      </div>
    </div>
  );
}

function Footer({
  step,
  setStep,
  finish,
}: {
  step: Step;
  setStep: (s: Step) => void;
  finish: () => void;
}) {
  return (
    <div className="bg-muted/20 flex shrink-0 items-center justify-between border-t border-border p-6">
      <Button
        type="button"
        variant="ghost"
        size="sm"
        onClick={() => setStep(Math.max(1, step - 1) as Step)}
        disabled={step === 1}
      >
        Back
      </Button>
      {step < 4 ? (
        <Button type="button" size="sm" onClick={() => setStep((step + 1) as Step)}>
          Continue
          <ArrowRight className="size-4" />
        </Button>
      ) : (
        <Button type="button" size="sm" onClick={finish}>
          Launch IDE
          <Check className="size-4" />
        </Button>
      )}
    </div>
  );
}

function StepOne() {
  return (
    <Section title="Welcome to Testing IDE">
      <p className="text-muted-foreground text-sm">
        Local-first IDE for generating test plans, test cases, and defect reports against your code
        with AI you control.
      </p>
      <ul className="mt-4 space-y-2 text-sm">
        <Bullet>Runs offline by default via Ollama.</Bullet>
        <Bullet>Bring your own OpenAI / Anthropic / OpenRouter key for cloud models.</Bullet>
        <Bullet>API keys stored encrypted at rest (AES-GCM); never logged.</Bullet>
      </ul>
    </Section>
  );
}

function StepTwo({
  status,
  error,
  tier,
}: {
  status: HealthStatus | null;
  error: string | null;
  tier: HardwareTier | null;
}) {
  return (
    <Section title="Hardware detection">
      <p className="text-muted-foreground text-sm">
        Detected from this machine — no telemetry leaves the renderer.
      </p>
      {error !== null ? (
        <p className="text-destructive mt-3 text-sm" role="alert">
          {error}
        </p>
      ) : status === null ? (
        <p className="text-muted-foreground mt-3 flex items-center gap-2 text-sm">
          <Loader2 className="size-3 animate-spin" /> Probing…
        </p>
      ) : (
        <div className="mt-4 grid grid-cols-2 gap-3">
          <Card icon={<Cpu className="size-4" />} label="OS">
            {status.osName} {status.osVersion}
          </Card>
          <Card icon={<Cpu className="size-4" />} label="CPUs">
            {status.cpuCount}
          </Card>
          <Card icon={<HardDrive className="size-4" />} label="Memory">
            {(status.totalMemoryMb / 1024).toFixed(1)} GB total
            <br />
            <span className="text-muted-foreground">
              {(status.availableMemoryMb / 1024).toFixed(1)} GB available
            </span>
          </Card>
          <Card icon={<Server className="size-4" />} label="Database">
            {status.dbOk ? 'reachable' : 'unreachable'}
          </Card>
        </div>
      )}
      {tier !== null ? (
        <div className="mt-4 rounded-lg border border-border bg-background p-3 text-sm">
          <p className="font-medium">Suggested tier: {tier.label}</p>
          <p className="text-muted-foreground mt-1 text-xs">{tier.rationale}</p>
        </div>
      ) : null}
    </Section>
  );
}

function StepThree() {
  const [result, setResult] = useState<ProviderConnectionTestResult | null>(null);
  const [pending, setPending] = useState(true);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const r = await providers.testProviderConnection({
          provider: 'ollama',
          baseUrl: 'http://localhost:11434',
        });
        if (!cancelled) setResult(r);
      } catch (err) {
        if (!cancelled) {
          setResult({
            ok: false,
            message: err instanceof IpcError ? err.message : String(err),
            latencyMs: 0,
            models: [],
          });
        }
      } finally {
        if (!cancelled) setPending(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  return (
    <Section title="Local AI engine">
      <p className="text-muted-foreground text-sm">
        Testing IDE uses Ollama for local inference. Install from{' '}
        <code className="bg-muted rounded px-1 text-xs">ollama.com</code> and run{' '}
        <code className="bg-muted rounded px-1 text-xs">ollama serve</code>.
      </p>
      <div className="mt-4 rounded-lg border border-border bg-background p-4">
        {pending ? (
          <p className="text-muted-foreground flex items-center gap-2 text-sm">
            <Loader2 className="size-3 animate-spin" /> Probing http://localhost:11434…
          </p>
        ) : result?.ok === true ? (
          <p className="flex items-center gap-2 text-sm text-green-600 dark:text-green-400">
            <Check className="size-4" /> {result.message}
            <span className="text-muted-foreground">({result.latencyMs} ms)</span>
          </p>
        ) : (
          <p className="text-destructive flex items-start gap-2 text-sm" role="alert">
            <X className="mt-0.5 size-4 shrink-0" />
            <span>
              {result?.message ?? 'Probe failed'}
              <br />
              <span className="text-muted-foreground text-xs">
                You can still continue and configure cloud providers in Settings.
              </span>
            </span>
          </p>
        )}
      </div>
    </Section>
  );
}

function StepFour({ tier }: { tier: HardwareTier | null }) {
  const recommended = tier?.recommendedModel ?? 'qwen2.5-coder:7b';
  const [model, setModel] = useState<string>(recommended);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [installedModels, setInstalledModels] = useState<string[] | null>(null);

  // Stay in sync if the tier loads after this step renders.
  useEffect(() => {
    setModel(recommended);
  }, [recommended]);

  // Probe Ollama for the locally-pulled model list so we can warn
  // when the user picks something they have not pulled yet.
  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const list = await providers.listOllamaModels('http://localhost:11434');
        if (!cancelled) setInstalledModels(list.map((m) => m.name));
      } catch {
        if (!cancelled) setInstalledModels([]);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  const isInstalled = installedModels !== null && installedModels.includes(model);
  const probeFailed = installedModels !== null && installedModels.length === 0;

  const save = useCallback(() => {
    setSaving(true);
    setError(null);
    void (async () => {
      try {
        await providers.saveProviderConfig({
          provider: 'ollama',
          baseUrl: 'http://localhost:11434',
          defaultModel: model,
          isActive: true,
        });
        setSaved(model);
      } catch (err) {
        setError(err instanceof IpcError ? err.message : String(err));
      } finally {
        setSaving(false);
      }
    })();
  }, [model]);

  return (
    <Section title="Pick a default model">
      <p className="text-muted-foreground text-sm">
        Saves an Ollama provider config so the AI panel works on first launch. Add cloud providers
        anytime in Settings.
      </p>
      <div className="mt-4 space-y-3">
        <ModelOption
          model="qwen2.5-coder:7b"
          label="Qwen 2.5 Coder 7B"
          hint="Default. ~4.7 GB. Runs on 8 GB VRAM or 16 GB RAM CPU."
          checked={model === 'qwen2.5-coder:7b'}
          onChoose={() => setModel('qwen2.5-coder:7b')}
          recommended={recommended === 'qwen2.5-coder:7b'}
        />
        <ModelOption
          model="qwen2.5-coder:1.5b"
          label="Qwen 2.5 Coder 1.5B"
          hint="Smaller. CPU-only on modest hardware. Slower but lighter."
          checked={model === 'qwen2.5-coder:1.5b'}
          onChoose={() => setModel('qwen2.5-coder:1.5b')}
          recommended={recommended === 'qwen2.5-coder:1.5b'}
        />
        <ModelOption
          model="qwen2.5-coder:14b"
          label="Qwen 2.5 Coder 14B"
          hint="Better quality. Needs ~12-16 GB VRAM (RTX 4070 Ti / M2 Pro)."
          checked={model === 'qwen2.5-coder:14b'}
          onChoose={() => setModel('qwen2.5-coder:14b')}
          recommended={recommended === 'qwen2.5-coder:14b'}
        />
      </div>
      {error !== null ? (
        <p className="text-destructive mt-3 text-xs" role="alert">
          {error}
        </p>
      ) : null}
      {installedModels !== null && !isInstalled ? (
        <div className="mt-3 rounded-md border border-yellow-500/30 bg-yellow-500/5 p-2.5 text-xs">
          <p className="text-yellow-700 dark:text-yellow-400">
            {probeFailed
              ? 'Ollama is unreachable. Start the daemon then run:'
              : `Model not pulled yet. Run:`}
          </p>
          <code className="bg-muted text-foreground mt-1.5 block rounded px-2 py-1.5 font-mono text-[11px]">
            ollama pull {model}
          </code>
        </div>
      ) : null}
      {saved !== null ? (
        <p className="mt-3 text-xs text-green-600 dark:text-green-400">Saved {saved} ✓</p>
      ) : null}
      <div className="mt-4">
        <Button type="button" size="sm" variant="outline" onClick={save} disabled={saving}>
          {saving ? <Loader2 className="size-3.5 animate-spin" /> : null}
          {saved === model ? 'Saved' : 'Save selection'}
        </Button>
      </div>
    </Section>
  );
}

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <div className="space-y-2">
      <h2 className="text-2xl font-semibold tracking-tight">{title}</h2>
      {children}
    </div>
  );
}

function Bullet({ children }: { children: ReactNode }) {
  return (
    <li className="flex items-start gap-2 text-sm">
      <Check className="mt-0.5 size-4 shrink-0 text-green-500" />
      {children}
    </li>
  );
}

function Card({ icon, label, children }: { icon: ReactNode; label: string; children: ReactNode }) {
  return (
    <div className="bg-background flex items-start gap-3 rounded-lg border border-border p-3">
      <span className="text-primary mt-0.5">{icon}</span>
      <div className="min-w-0 text-sm">
        <p className="text-muted-foreground text-xs uppercase tracking-wider">{label}</p>
        <div className="mt-0.5">{children}</div>
      </div>
    </div>
  );
}

function ModelOption({
  model,
  label,
  hint,
  checked,
  onChoose,
  recommended,
}: {
  model: string;
  label: string;
  hint: string;
  checked: boolean;
  onChoose: () => void;
  recommended: boolean;
}) {
  return (
    <label
      className={`flex cursor-pointer items-start justify-between rounded-lg border p-3 transition-colors ${
        checked ? 'border-primary bg-primary/5' : 'border-border bg-card hover:bg-muted/50'
      }`}
    >
      <div className="min-w-0">
        <div className="flex items-center gap-2 text-sm font-medium">
          {label}
          {recommended ? (
            <span className="bg-primary text-primary-foreground rounded px-1.5 py-0.5 text-[9px] font-bold uppercase tracking-wider">
              Recommended
            </span>
          ) : null}
        </div>
        <p className="text-muted-foreground mt-0.5 text-xs">{hint}</p>
        <code className="text-muted-foreground mt-1 block text-[10px]">{model}</code>
      </div>
      <input
        type="radio"
        name="default-model"
        value={model}
        checked={checked}
        onChange={onChoose}
        className="sr-only"
      />
      <span
        className={`mt-1 size-4 shrink-0 rounded-full border-2 ${
          checked ? 'border-primary bg-primary/30' : 'border-border'
        }`}
      />
    </label>
  );
}
