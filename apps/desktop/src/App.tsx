import { invoke } from '@tauri-apps/api/core';
import { LoginSchema, RegisterSchema } from '@testing-ide/shared';
import { useCallback, useEffect, useState } from 'react';
import type { ZodError } from 'zod';

import { AiPanel } from '@/components/ai-panel/ai-panel';
import { EditorPanel } from '@/components/editor/editor-panel';
import { FileExplorer } from '@/components/file-explorer/file-explorer';
import { FirstRunWizard } from '@/components/first-run-wizard';
import { AppShell } from '@/components/layout/app-shell';
import { SettingsSheet } from '@/components/settings/settings-sheet';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { IpcError, system } from '@/lib/ipc';
import type { InitDbResponse } from '@/lib/ipc/system';
import { readOnboardingFlag } from '@/lib/onboarding';
import { useAuthStore } from '@/stores/auth-store';

type TokenPair = {
  accessToken: string;
  refreshToken: string;
  tokenType: string;
};

type SessionUser = {
  id: string;
  email: string;
  name: string | null;
};

function formatZodError(err: ZodError): string {
  const flat = err.flatten();
  const parts: string[] = [];
  for (const [key, msgs] of Object.entries(flat.fieldErrors)) {
    if (Array.isArray(msgs) && msgs.length > 0) {
      parts.push(`${key}: ${msgs.join(', ')}`);
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

/**
 * Desktop shell.
 *
 * Phase 9: three-panel workspace (explorer + editor + AI panel) once
 * the first-run wizard is dismissed. Auth + DB-smoke controls move into
 * a floating dev panel so they stay accessible without owning the
 * whole window.
 */
export function App() {
  const [showWizard, setShowWizard] = useState<boolean>(() => !readOnboardingFlag());
  const [showDevPanel, setShowDevPanel] = useState(false);

  if (showWizard) {
    return <FirstRunWizard onComplete={() => setShowWizard(false)} />;
  }

  return (
    <>
      <AppShell sidebar={<FileExplorer />} editor={<EditorPanel />} aiPanel={<AiPanel />} />
      <SettingsSheet />
      <DevPanelToggle open={showDevPanel} onToggle={() => setShowDevPanel((v) => !v)} />
      {showDevPanel ? <DevPanel /> : null}
    </>
  );
}

function DevPanelToggle({ open, onToggle }: { open: boolean; onToggle: () => void }) {
  return (
    <button
      type="button"
      onClick={onToggle}
      className="fixed bottom-8 right-3 z-40 rounded-md border border-border bg-card px-2 py-1 text-[10px] font-mono text-muted-foreground hover:text-foreground"
      aria-label="Toggle developer panel"
    >
      {open ? 'hide dev' : 'dev'}
    </button>
  );
}

function DevPanel() {
  const [initResult, setInitResult] = useState<InitDbResponse | null>(null);
  const [initError, setInitError] = useState<string | null>(null);
  const [greeting, setGreeting] = useState<string | null>(null);
  const [greetError, setGreetError] = useState<string | null>(null);

  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [name, setName] = useState('');
  const [authError, setAuthError] = useState<string | null>(null);
  const [sessionUser, setSessionUser] = useState<SessionUser | null>(null);

  const accessToken = useAuthStore((s) => s.accessToken);
  const refreshToken = useAuthStore((s) => s.refreshToken);
  const setTokens = useAuthStore((s) => s.setTokens);
  const clearAuth = useAuthStore((s) => s.clear);

  useEffect(() => {
    let cancelled = false;
    void system
      .initDb()
      .then((r) => {
        if (!cancelled) setInitResult(r);
      })
      .catch((err: unknown) => {
        if (cancelled) return;
        setInitError(err instanceof IpcError ? err.message : String(err));
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const handleGreet = useCallback(() => {
    setGreetError(null);
    void system
      .greet('Testing IDE')
      .then((msg) => {
        setGreeting(msg);
      })
      .catch((err: unknown) => {
        setGreetError(err instanceof IpcError ? err.message : String(err));
      });
  }, []);

  const handleRegister = useCallback(() => {
    setAuthError(null);
    const parsed = RegisterSchema.safeParse({
      email,
      password,
      name: name.trim() === '' ? undefined : name.trim(),
    });
    if (!parsed.success) {
      setAuthError(formatZodError(parsed.error));
      return;
    }
    void invoke<TokenPair>('register', { body: parsed.data })
      .then((pair) => {
        setTokens(pair.accessToken, pair.refreshToken);
        setSessionUser(null);
      })
      .catch((err: unknown) => {
        setAuthError(err instanceof Error ? err.message : String(err));
      });
  }, [email, password, name, setTokens]);

  const handleLogin = useCallback(() => {
    setAuthError(null);
    const parsed = LoginSchema.safeParse({ email, password });
    if (!parsed.success) {
      setAuthError(formatZodError(parsed.error));
      return;
    }
    void invoke<TokenPair>('login', { body: parsed.data })
      .then((pair) => {
        setTokens(pair.accessToken, pair.refreshToken);
        setSessionUser(null);
      })
      .catch((err: unknown) => {
        setAuthError(err instanceof Error ? err.message : String(err));
      });
  }, [email, password, setTokens]);

  const handleRefresh = useCallback(() => {
    setAuthError(null);
    if (refreshToken === null) {
      setAuthError('No refresh token in session store');
      return;
    }
    void invoke<TokenPair>('refresh_token', { body: { refreshToken } })
      .then((pair) => {
        setTokens(pair.accessToken, pair.refreshToken);
        setSessionUser(null);
      })
      .catch((err: unknown) => {
        setAuthError(err instanceof Error ? err.message : String(err));
      });
  }, [refreshToken, setTokens]);

  const handleMe = useCallback(() => {
    setAuthError(null);
    if (accessToken === null) {
      setAuthError('No access token in session store');
      return;
    }
    void invoke<SessionUser>('auth_me', { authorization: `Bearer ${accessToken}` })
      .then((u) => {
        setSessionUser(u);
      })
      .catch((err: unknown) => {
        setAuthError(err instanceof Error ? err.message : String(err));
      });
  }, [accessToken]);

  return (
    <aside className="fixed bottom-16 right-3 z-40 max-h-[80vh] w-[360px] overflow-y-auto rounded-md border border-border bg-card p-3 shadow-lg">
      <h3 className="mb-3 text-xs font-semibold uppercase tracking-wider text-muted-foreground">
        Dev panel
      </h3>

      <section className="space-y-1">
        <h4 className="text-xs font-medium">Database</h4>
        {initError !== null ? (
          <p className="text-destructive text-xs" role="alert">
            {initError}
          </p>
        ) : initResult !== null ? (
          <p className="text-xs">
            <span className="text-muted-foreground">SQLite: </span>
            <code className="bg-muted truncate rounded px-1 py-0.5 text-[10px]">
              {initResult.dbPath}
            </code>
          </p>
        ) : (
          <p className="text-muted-foreground text-xs">Initializing…</p>
        )}
      </section>

      <section className="mt-3 space-y-2">
        <h4 className="text-xs font-medium">Auth</h4>
        <div className="grid gap-1.5">
          <Input
            placeholder="email"
            value={email}
            onChange={(e) => {
              setEmail(e.target.value);
            }}
          />
          <Input
            type="password"
            placeholder="password"
            value={password}
            onChange={(e) => {
              setPassword(e.target.value);
            }}
          />
          <Input
            placeholder="display name (register)"
            value={name}
            onChange={(e) => {
              setName(e.target.value);
            }}
          />
        </div>
        {authError !== null ? (
          <p className="text-destructive text-xs" role="alert">
            {authError}
          </p>
        ) : null}
        <div className="flex flex-wrap gap-1">
          <Button size="sm" variant="secondary" onClick={handleRegister}>
            Register
          </Button>
          <Button size="sm" onClick={handleLogin}>
            Login
          </Button>
          <Button size="sm" variant="outline" onClick={handleRefresh}>
            Refresh
          </Button>
          <Button size="sm" variant="outline" onClick={handleMe}>
            Me
          </Button>
          <Button
            size="sm"
            variant="ghost"
            onClick={() => {
              clearAuth();
              setSessionUser(null);
            }}
          >
            Clear
          </Button>
        </div>
        <div className="text-muted-foreground space-y-0.5 text-[10px]">
          <p>
            Token:{' '}
            {accessToken === null ? (
              '—'
            ) : (
              <code className="break-all">{`${accessToken.slice(0, 24)}…`}</code>
            )}
          </p>
          {sessionUser !== null ? (
            <p>
              Session: <code>{sessionUser.email}</code>
            </p>
          ) : null}
        </div>
      </section>

      <section className="mt-3 space-y-1">
        <h4 className="text-xs font-medium">Smoke</h4>
        <div className="flex items-center gap-2">
          <Button size="sm" variant="outline" onClick={handleGreet}>
            greet
          </Button>
          {greetError !== null ? (
            <span className="text-destructive text-xs" role="alert">
              {greetError}
            </span>
          ) : null}
          {greeting !== null ? <span className="text-xs">{greeting}</span> : null}
        </div>
      </section>
    </aside>
  );
}
