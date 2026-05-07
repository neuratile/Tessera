import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import {
  type GenerationStreamEvent,
  GenerationStreamEventSchema,
} from '@testing-ide/shared';

import { IpcError } from './error';

/**
 * Channel name backend emits on. Mirrored from
 * `commands/generation.rs::GENERATION_EVENT`.
 */
const GENERATION_EVENT_CHANNEL = 'generation://event';

/**
 * Subscribe to generation streaming events. Returns an `unlisten`
 * callback that the caller MUST invoke when the consumer unmounts —
 * Tauri's `listen` registers a renderer-side handler that survives
 * across re-renders otherwise.
 *
 * Each incoming payload is validated against
 * `GenerationStreamEventSchema`. Schema-invalid payloads are dropped
 * silently (a future backend bump that ships an unknown `kind` should
 * not crash the renderer).
 */
export async function subscribeToGenerationEvents(
  handler: (event: GenerationStreamEvent) => void,
): Promise<UnlistenFn> {
  try {
    return await listen<unknown>(GENERATION_EVENT_CHANNEL, (event) => {
      const parsed = GenerationStreamEventSchema.safeParse(event.payload);
      if (parsed.success) {
        handler(parsed.data);
      }
    });
  } catch (err) {
    throw new IpcError(GENERATION_EVENT_CHANNEL, asMessage(err), { cause: err });
  }
}

function asMessage(err: unknown): string {
  if (err instanceof Error) return err.message;
  if (typeof err === 'string') return err;
  return JSON.stringify(err);
}
