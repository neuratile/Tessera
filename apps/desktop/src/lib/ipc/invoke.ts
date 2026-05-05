import { invoke } from '@tauri-apps/api/core';
import type { z } from 'zod';

import { IpcError, asMessage } from './error';

/**
 * Internal helper used by every domain wrapper.
 *
 * - Calls the named Tauri command with `args`.
 * - Validates the response against the supplied Zod schema.
 * - Wraps any thrown error in `IpcError` so callers always see a typed
 *   exception with the originating command name attached.
 */
export async function invokeAndParse<S extends z.ZodTypeAny>(
  command: string,
  schema: S,
  args?: Record<string, unknown>,
): Promise<z.infer<S>> {
  try {
    const raw = await invoke<unknown>(command, args);
    const result = schema.safeParse(raw);
    if (!result.success) {
      throw new IpcError(command, `response failed schema validation: ${result.error.message}`);
    }
    return result.data as z.infer<S>;
  } catch (err) {
    if (err instanceof IpcError) throw err;
    throw new IpcError(command, asMessage(err), { cause: err });
  }
}

/**
 * Variant for commands whose return type is `void` / no body
 * (e.g. `delete_project`). Skips schema validation.
 */
export async function invokeVoid(
  command: string,
  args?: Record<string, unknown>,
): Promise<void> {
  try {
    await invoke<void>(command, args);
  } catch (err) {
    throw new IpcError(command, asMessage(err), { cause: err });
  }
}

/**
 * Variant for commands whose return type is a plain string.
 */
export async function invokeString(
  command: string,
  args?: Record<string, unknown>,
): Promise<string> {
  try {
    return await invoke<string>(command, args);
  } catch (err) {
    throw new IpcError(command, asMessage(err), { cause: err });
  }
}
