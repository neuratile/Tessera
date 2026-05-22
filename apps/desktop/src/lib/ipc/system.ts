import { z } from 'zod';

import { invokeAndParse, invokeString, invokeVoid } from './invoke';

const InitDbResponseSchema = z.object({
  dbPath: z.string(),
  ok: z.boolean(),
});

export type InitDbResponse = z.infer<typeof InitDbResponseSchema>;

/** Phase 1 smoke command — confirms DB pool reachability. */
export async function initDb(): Promise<InitDbResponse> {
  return invokeAndParse('init_db', InitDbResponseSchema);
}

/** Phase 1 sanity command — round-trips a string through the IPC bridge. */
export async function greet(name: string): Promise<string> {
  return invokeString('greet', { name });
}

/**
 * Forward a frontend warning/error into the Rust-side tracing subscriber.
 * The renderer must not call `console.*` (rules.md "no console.log in
 * frontend"); this command is the supported channel for surfacing
 * browser-context failures into the structured log stream.
 *
 * Best-effort: errors from the IPC call itself are swallowed because
 * the caller is typically already inside an error path and re-throwing
 * would mask the original failure.
 */
export async function logToBackend(
  level: 'warn' | 'error',
  source: string,
  message: string,
): Promise<void> {
  try {
    await invokeVoid('frontend_log', { level, source, message });
  } catch {
    // Logging must not throw — preserve the caller's original error path.
  }
}
