import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import {
  type HealRequest,
  HealRequestSchema,
  type HealResult,
  HealResultSchema,
  type HealStreamEvent,
  HealStreamEventSchema,
} from '@testing-ide/shared';

import { IpcError } from './error';
import { invokeAndParse } from './invoke';

/**
 * Channel the backend emits per-attempt heal progress on. Mirrored from
 * `commands/healing.rs::HEAL_EVENT`.
 */
const HEAL_EVENT_CHANNEL = 'heal://event';

/**
 * Run the bounded self-healing loop over a test-cases artifact
 * (plan/versions/v2/v2-feature-docs/SELF_HEALING_LOOP.md). Validates `request`
 * against `HealRequestSchema` before sending so callers fail fast on bad input.
 * `maxAttempts` is a hint — the backend re-clamps it to [1, 5].
 *
 * A runner-level failure, a cancellation mid-loop, or a regeneration error is
 * **not** an exception — it comes back as a `HealResult` with `outcome:
 * 'error'` carrying an `errorMessage`. Only a pre-flight rejection (opt-out,
 * blank/missing artifact, unresolvable provider) throws an `IpcError`.
 */
export async function runSelfHeal(request: HealRequest): Promise<HealResult> {
  const parsed = HealRequestSchema.safeParse(request);
  if (!parsed.success) {
    throw new IpcError('run_self_heal', `invalid arguments: ${parsed.error.message}`);
  }
  return invokeAndParse('run_self_heal', HealResultSchema, { request: parsed.data });
}

/**
 * Subscribe to per-attempt heal progress events. Returns an `unlisten`
 * callback the caller MUST invoke on unmount. Schema-invalid payloads are
 * dropped silently so a future backend event kind cannot crash the renderer.
 */
export async function subscribeToHealEvents(
  handler: (event: HealStreamEvent) => void,
): Promise<UnlistenFn> {
  try {
    return await listen<unknown>(HEAL_EVENT_CHANNEL, (event) => {
      const parsed = HealStreamEventSchema.safeParse(event.payload);
      if (parsed.success) {
        handler(parsed.data);
      }
    });
  } catch (err) {
    throw new IpcError(HEAL_EVENT_CHANNEL, asMessage(err), { cause: err });
  }
}

function asMessage(err: unknown): string {
  if (err instanceof Error) return err.message;
  if (typeof err === 'string') return err;
  return JSON.stringify(err);
}
