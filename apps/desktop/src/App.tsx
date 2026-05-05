import { invoke } from '@tauri-apps/api/core';
import { LoginSchema, RegisterSchema } from '@testing-ide/shared';
import { useCallback, useEffect, useState } from 'react';
import type { ZodError } from 'zod';

import { FirstRunWizard } from '@/components/first-run-wizard';
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
 * Phase 8: routes to the first-run wizard until the user dismisses it,
 * then renders the IPC smoke panel + Phase 5 auth panel. Real workspace
 * UI (file tree, Monaco, AI panel) lands in later phases.
 */
export function App() {
  const [showWizard, setShowWizard] = useState<boolean>(() => !readOnboardingFlag());
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
    if (showWizard) return;
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
  }, [showWizard]);

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

  if (showWizard) {
    return <FirstRunWizard onComplete={() => setShowWizard(false)} />;
  }

  return (
    <div className="flex min-h-screen flex-col gap-6 p-8">
      <header className="space-y-1">
        <h1 className="text-2xl font-semibold tracking-tight">Testing IDE</h1>
        <p className="text-muted-foreground text-sm">Tauri 2 + Vite + React + Tailwind v4</p>
      </header>

      <section className="space-y-2 rounded-lg border border-border p-4">
        <h2 className="text-sm font-medium">Database</h2>
        {initError ? (
          <p className="text-destructive text-sm" role="alert">
            {initError}
          </p>
        ) : null}
        {initResult ? (
          <p className="text-sm">
            <span className="text-muted-foreground">SQLite: </span>
            <code className="rounded bg-muted px-1 py-0.5 text-xs">{initResult.dbPath}</code>
          </p>
        ) : (
          <p className="text-muted-foreground text-sm">Initializing…</p>
        )}
      </section>

      <section className="space-y-3 rounded-lg border border-border p-4">
        <h2 className="text-sm font-medium">Auth (Phase 5)</h2>
        <p className="text-muted-foreground text-xs">
          Tokens stay in memory (Zustand) until you reload. Set a strong{' '}
          <code className="text-xs">JWT_SECRET</code> for real builds.
        </p>
        <div className="grid max-w-md gap-2">
          <label className="text-xs font-medium" htmlFor="auth-email">
            Email
          </label>
          <Input
            id="auth-email"
            autoComplete="email"
            value={email}
            onChange={(e) => {
              setEmail(e.target.value);
            }}
          />
          <label className="text-xs font-medium" htmlFor="auth-password">
            Password
          </label>
          <Input
            id="auth-password"
            type="password"
            autoComplete="current-password"
            value={password}
            onChange={(e) => {
              setPassword(e.target.value);
            }}
          />
          <label className="text-xs font-medium" htmlFor="auth-name">
            Display name (register only)
          </label>
          <Input
            id="auth-name"
            value={name}
            onChange={(e) => {
              setName(e.target.value);
            }}
          />
        </div>
        {authError ? (
          <p className="text-destructive text-sm" role="alert">
            {authError}
          </p>
        ) : null}
        <div className="flex flex-wrap gap-2">
          <Button type="button" variant="secondary" onClick={handleRegister}>
            Register
          </Button>
          <Button type="button" onClick={handleLogin}>
            Login
          </Button>
          <Button type="button" variant="outline" onClick={handleRefresh}>
            Refresh token
          </Button>
          <Button type="button" variant="outline" onClick={handleMe}>
            Session (auth_me)
          </Button>
          <Button
            type="button"
            variant="ghost"
            onClick={() => {
              clearAuth();
              setSessionUser(null);
            }}
          >
            Clear tokens
          </Button>
        </div>
        <div className="text-muted-foreground space-y-1 text-xs">
          <p>
            Access token:{' '}
            {accessToken === null ? (
              '—'
            ) : (
              <code className="break-all">{`${accessToken.slice(0, 24)}…`}</code>
            )}
          </p>
          {sessionUser ? (
            <p>
              Session: <code>{sessionUser.email}</code> ({sessionUser.id.slice(0, 8)}…)
            </p>
          ) : null}
        </div>
      </section>

      <section className="flex flex-wrap items-center gap-3">
        <Button type="button" onClick={handleGreet}>
          Call greet command
        </Button>
        {greetError ? (
          <span className="text-destructive text-sm" role="alert">
            {greetError}
          </span>
        ) : null}
        {greeting ? <span className="text-sm">{greeting}</span> : null}
      </section>
    </div>
  );
}
