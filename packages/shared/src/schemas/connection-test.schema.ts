import { z } from 'zod';

import { LlmProviderIdSchema } from './llm-provider.schema';

/**
 * Arguments accepted by `test_provider_connection` — mirrors
 * `ConnectionTestArgs` in `commands/providers.rs`. The `apiKey` is held
 * in renderer memory only for the duration of the call; it is never
 * persisted by this command (`save_provider_config` does that, with
 * AES-GCM at rest).
 */
export const ProviderConnectionTestArgsSchema = z.object({
  provider: LlmProviderIdSchema,
  apiKey: z.string().optional(),
  baseUrl: z.string().optional(),
});

export type ProviderConnectionTestArgs = z.infer<typeof ProviderConnectionTestArgsSchema>;

/**
 * Result of `test_provider_connection` — mirrors
 * `ProviderConnectionTestResult` in
 * `apps/desktop/src-tauri/src/services/provider_connection_service.rs`.
 *
 * `models` is the list of remote / local models reachable with the
 * supplied credentials; empty when the probe failed or the provider's
 * model-list endpoint is not exercised (e.g. cloud construction-only
 * checks).
 */
export const ProviderConnectionTestResultSchema = z.object({
  ok: z.boolean(),
  message: z.string(),
  latencyMs: z.number().int().nonnegative(),
  models: z.array(z.string()).default([]),
});

export type ProviderConnectionTestResult = z.infer<typeof ProviderConnectionTestResultSchema>;
