import type { HealthStatus, ProviderConnectionTestResult } from '@testing-ide/shared';
import { ArrowRight, Check, Cpu, HardDrive, Loader2, Server, X } from 'lucide-react';
import type { ReactNode } from 'react';
import { useCallback, useEffect, useState } from 'react';

import { Button } from '@/components/ui/button';
import { recommendTier, type HardwareTier } from '@/lib/hardware-tier';
import { getErrorMessage, health, providers } from '@/lib/ipc';
import { markOnboardingComplete } from '@/lib/onboarding';

type Props = {
  /** Called once the user dismisses the wizard. Parent should re-render. */
  onComplete: () => void;
};

type Step = 1 | 2 | 3 | 4;

function previousStep(step: Step): Step {
  switch (step) {
    case 1:
      return 1;
    case 2:
      return 1;
    case 3:
      return 2;
    case 4:
      return 3;
  }
}

function nextStep(step: Step): Step {
  switch (step) {
    case 1:
      return 2;
    case 2:
      return 3;
    case 3:
      return 4;
    case 4:
      return 4;
  }
}

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
  const [modelReady, setModelReady] = useState(false);
  const [modelSaving, setModelSaving] = useState(false);
  const [savedModel, setSavedModel] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    void health
      .healthCheck()
      .then((s) => {
        if (!cancelled) setStatus(s);
      })
      .catch((err: unknown) => {
        if (cancelled) return;
        setHealthError(getErrorMessage(err));
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
    <div className="bg-background relative flex h-screen w-screen items-center justify-center p-4">
      <div className="bg-mosaic" aria-hidden="true" />
      <div className="bg-card relative z-10 flex h-[540px] max-h-full w-full max-w-2xl flex-col overflow-hidden rounded-xl border border-border shadow-2xl">
        <Header step={step} />
        <div className="flex-1 overflow-y-auto p-8">
          {step === 1 && <StepOne />}
          {step === 2 && <StepTwo status={status} error={healthError} tier={tier} />}
          {step === 3 && <StepThree />}
          {step === 4 && <StepFour tier={tier} savedModel={savedModel} onSaved={setSavedModel} onReadyChange={setModelReady} onSavingChange={setModelSaving} />}
        </div>
        <Footer step={step} setStep={setStep} finish={finish} modelReady={modelReady} modelSaving={modelSaving} />
      </div>
    </div>
  );
}

function Header({ step }: { step: Step }) {
  const labels = ['Welcome', 'Your machine', 'Local AI', 'Model'];
  return (
    <div className="bg-surface-3 shrink-0 border-b border-border p-6">
      <div className="mb-3 flex items-center justify-between gap-3">
        <h1 className="flex items-center gap-3">
          <img
            src="/tessera-logo.png"
            alt="Tessera"
            className="size-8 rounded-md shrink-0"
            draggable="false"
          />
          <span className="flex items-baseline gap-2">
            <span className="font-brand text-primary text-lg">tessera</span>
            <span className="text-[10px] font-semibold uppercase tracking-[0.16em] text-muted-foreground">
              quick setup · {step} of 4
            </span>
          </span>
        </h1>
        <span className="text-muted-foreground text-[10px] font-mono">
          {labels[step - 1]}
        </span>
      </div>
      <div className="flex gap-1.5">
        {[1, 2, 3, 4].map((s) => (
          <div
            key={s}
            className={`h-1 flex-1 rounded-full transition-colors ${
              step >= s ? 'bg-primary' : 'bg-surface-2'
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
  modelReady,
  modelSaving,
}: {
  step: Step;
  setStep: (s: Step) => void;
  finish: () => void;
  modelReady: boolean;
  modelSaving: boolean;
}) {
  return (
    <div className="bg-surface-3 flex shrink-0 items-center justify-between border-t border-border px-6 py-4">
      <Button
        type="button"
        variant="ghost"
        size="sm"
        onClick={() => setStep(previousStep(step))}
        disabled={step === 1 || modelSaving}
      >
        Back
      </Button>
      {step < 4 ? (
        <div className="flex items-center gap-2">
          <Button
            type="button"
            variant="ghost"
            size="sm"
            className="text-muted-foreground"
            onClick={finish}
          >
            Skip for now
          </Button>
          <Button type="button" size="sm" onClick={() => setStep(nextStep(step))}>
            Continue
            <ArrowRight className="size-4" />
          </Button>
        </div>
      ) : (
        <Button type="button" size="sm" onClick={finish} disabled={modelSaving}>
          {modelReady ? 'Start using Tessera' : 'Continue without AI'}
          <Check className="size-4" />
        </Button>
      )}
    </div>
  );
}

function StepOne() {
  return (
    <Section title="Welcome to Tessera">
      <p className="text-muted-foreground text-sm">
        Turn your code into test plans, test cases, and bug reports — with AI that runs entirely
        on your machine.
      </p>
      <ul className="mt-4 space-y-2 text-sm">
        <Bullet>Private by default — your code never leaves this computer.</Bullet>
        <Bullet>Free local AI via Ollama, or bring your own OpenAI / Anthropic / Gemini key.</Bullet>
        <Bullet>Setup takes about a minute, and everything can be changed later in Settings.</Bullet>
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
    <Section title="Let's check your computer">
      <p className="text-muted-foreground text-sm">
        A quick look at your hardware so we can suggest an AI model that runs smoothly here.
        Nothing is sent anywhere.
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
          <p className="font-medium">Best fit for this machine: {tier.label}</p>
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
            message: getErrorMessage(err),
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
    <Section title="Connect your local AI">
      <p className="text-muted-foreground text-sm">
        Tessera uses Ollama to run AI models privately on your computer — free, no account
        needed. Don't have it yet? Download it from{' '}
        <code className="bg-muted rounded px-1 text-xs">ollama.com</code> and open it.
      </p>
      <div className="mt-4 rounded-md border border-border bg-background p-4">
        {pending ? (
          <p className="text-muted-foreground flex items-center gap-2 text-sm">
            <Loader2 className="size-3 animate-spin" /> Looking for Ollama…
          </p>
        ) : result?.ok === true ? (
          <p className="text-success flex items-center gap-2 text-sm">
            <Check className="size-4" /> {result.message}
            <span className="text-muted-foreground">({result.latencyMs} ms)</span>
          </p>
        ) : (
          <p className="flex items-start gap-2 text-sm" role="alert">
            <X className="text-warning mt-0.5 size-4 shrink-0" />
            <span>
              Ollama isn't running yet — that's okay.
              <br />
              <span className="text-muted-foreground text-xs">
                You can finish setup now and connect it later from Settings, or use a cloud
                provider instead.
              </span>
            </span>
          </p>
        )}
      </div>
    </Section>
  );
}

function StepFour({ tier, savedModel, onSaved, onReadyChange, onSavingChange }: {
  tier: HardwareTier | null;
  savedModel: string | null;
  onSaved: (model: string) => void;
  onReadyChange: (ready: boolean) => void;
  onSavingChange: (saving: boolean) => void;
}) {
  const recommended = tier?.recommendedModel ?? 'qwen2.5-coder:7b';
  const [model, setModel] = useState<string>(savedModel ?? recommended);
  const [saving, setSaving] = useState(false);
  const saved = savedModel;
  const [error, setError] = useState<string | null>(null);
  const [installedModels, setInstalledModels] = useState<string[] | null>(null);

  // Stay in sync if the tier loads after this step renders.
  useEffect(() => {
    if (savedModel === null) setModel(recommended);
  }, [recommended, savedModel]);

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

  useEffect(() => {
    onReadyChange(saved === model && !saving);
    onSavingChange(saving);
  }, [saved, model, saving, onReadyChange, onSavingChange]);

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
        onSaved(model);
      } catch (err) {
        setError(getErrorMessage(err));
      } finally {
        setSaving(false);
      }
    })();
  }, [model, onSaved]);

  return (
    <Section title="Choose your AI model">
      <p className="text-muted-foreground text-sm">
        We've highlighted the best fit for your hardware. Select Use this model to save it,
        or continue without AI and configure it later in Settings.
      </p>
      <div className="mt-4 space-y-3">
        <ModelOption
          model="qwen2.5-coder:7b"
          label="Balanced"
          hint="Good speed and quality for most computers. Needs ~8 GB of free memory."
          checked={model === 'qwen2.5-coder:7b'}
          onChoose={() => setModel('qwen2.5-coder:7b')}
          recommended={recommended === 'qwen2.5-coder:7b'}
        />
        <ModelOption
          model="qwen2.5-coder:1.5b"
          label="Light"
          hint="Smallest and fastest to set up — ideal for laptops with 8 GB RAM or less."
          checked={model === 'qwen2.5-coder:1.5b'}
          onChoose={() => setModel('qwen2.5-coder:1.5b')}
          recommended={recommended === 'qwen2.5-coder:1.5b'}
        />
        <ModelOption
          model="qwen2.5-coder:14b"
          label="Quality"
          hint="Best results, but needs a powerful GPU (12 GB+ VRAM)."
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
        <div className="border-warning/30 bg-warning/5 mt-3 rounded-md border p-2.5 text-xs">
          <p className="text-warning">
            {probeFailed
              ? 'Ollama is not running. Once it is, download this model with:'
              : 'One more step — download this model by running:'}
          </p>
          <code className="bg-muted text-foreground mt-1.5 block rounded px-2 py-1.5 font-mono text-[11px]">
            ollama pull {model}
          </code>
        </div>
      ) : null}
      {saved !== null ? (
        <p className="text-success mt-3 text-xs">All set — {saved} is your default model ✓</p>
      ) : null}
      <div className="mt-4">
        <Button type="button" size="sm" variant="outline" onClick={save} disabled={saving}>
          {saving ? <Loader2 className="size-3.5 animate-spin" /> : null}
          {saved === model ? 'Saved' : 'Use this model'}
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
      <Check className="text-primary mt-0.5 size-4 shrink-0" />
      {children}
    </li>
  );
}

function Card({ icon, label, children }: { icon: ReactNode; label: string; children: ReactNode }) {
  return (
    <div className="bg-background flex items-start gap-3 rounded-md border border-border p-3">
      <span className="text-primary mt-0.5">{icon}</span>
      <div className="min-w-0 text-sm">
        <p className="text-muted-foreground text-[10px] font-semibold uppercase tracking-[0.12em]">
          {label}
        </p>
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
      className={`flex cursor-pointer items-start justify-between rounded-md border p-3 transition-colors ${
        checked
          ? 'border-primary bg-primary/8'
          : 'border-border bg-background hover:bg-muted/50 hover:border-primary/40'
      }`}
    >
      <div className="min-w-0">
        <div className="flex items-center gap-2 text-sm font-medium text-foreground">
          {label}
          {recommended ? (
            <span className="bg-primary text-primary-foreground rounded-sm px-1.5 py-0.5 text-[9px] font-bold uppercase tracking-[0.08em]">
              Recommended
            </span>
          ) : null}
        </div>
        <p className="text-muted-foreground mt-0.5 text-xs">{hint}</p>
        <code className="text-muted-foreground mt-1 block font-mono text-[10px]">{model}</code>
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
        className={`mt-1 size-4 shrink-0 rounded-full border-2 transition-colors ${
          checked ? 'border-primary bg-primary/40' : 'border-border'
        }`}
      />
    </label>
  );
}
