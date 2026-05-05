/**
 * Error type surfaced by every IPC wrapper in this directory.
 *
 * The Tauri backend returns `Result<T, String>` per `rules.md` §4.2.1
 * (the IPC bridge cannot serialize structured Rust errors). Wrappers
 * normalize the rejection into an `IpcError` so callers don't have to
 * `instanceof Error` everywhere.
 */
export class IpcError extends Error {
  /** The Tauri command name that produced this error. */
  readonly command: string;

  constructor(command: string, message: string, options?: { cause?: unknown }) {
    super(`[${command}] ${message}`, options);
    this.name = 'IpcError';
    this.command = command;
  }
}

/** Convert any unknown rejection into a string suitable for `IpcError`. */
export function asMessage(err: unknown): string {
  if (err instanceof Error) return err.message;
  if (typeof err === 'string') return err;
  return JSON.stringify(err);
}
