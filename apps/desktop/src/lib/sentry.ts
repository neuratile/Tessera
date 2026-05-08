import * as Sentry from '@sentry/react';
import { z } from 'zod';

const SentryEnvSchema = z.object({
  VITE_SENTRY_DSN: z.string().trim().min(1).optional(),
});

function readSentryDsn(): string | null {
  const parsed = SentryEnvSchema.safeParse(import.meta.env);
  if (!parsed.success) {
    return null;
  }
  return parsed.data.VITE_SENTRY_DSN ?? null;
}

/**
 * Initializes browser-side Sentry when a public DSN is configured.
 *
 * The desktop app ships without Sentry by default; local development stays
 * fully offline until `VITE_SENTRY_DSN` is set in `apps/desktop/.env`.
 */
export function initSentry(): void {
  const dsn = readSentryDsn();
  if (dsn === null) {
    return;
  }

  Sentry.init({
    dsn,
    environment: import.meta.env.MODE,
    release: `testing-ide-desktop@${__APP_VERSION__}`,
    sendDefaultPii: false,
  });
}
