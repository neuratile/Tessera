import { invoke } from '@tauri-apps/api/core';
import { useCallback, useEffect, useState } from 'react';

import { Button } from '@/components/ui/button';

type InitDbResponse = {
  dbPath: string;
  ok: boolean;
};

/**
 * Desktop shell: verifies Tauri IPC (`greet`, `init_db`) and renders a minimal layout.
 */
export function App() {
  const [initResult, setInitResult] = useState<InitDbResponse | null>(null);
  const [initError, setInitError] = useState<string | null>(null);
  const [greeting, setGreeting] = useState<string | null>(null);
  const [greetError, setGreetError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    void invoke<InitDbResponse>('init_db')
      .then((r) => {
        if (!cancelled) {
          setInitResult(r);
        }
      })
      .catch((err: unknown) => {
        if (!cancelled) {
          setInitError(err instanceof Error ? err.message : String(err));
        }
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const handleGreet = useCallback(() => {
    setGreetError(null);
    void invoke<string>('greet', { name: 'Testing IDE' })
      .then((msg) => {
        setGreeting(msg);
      })
      .catch((err: unknown) => {
        setGreetError(err instanceof Error ? err.message : String(err));
      });
  }, []);

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
