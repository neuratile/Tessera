import { z } from 'zod';

import {
  ConnectionTestResultSchema,
  ConnectionTestSchema,
} from './provider.schema';

/**
 * Legacy alias kept for compatibility with older imports.
 *
 * Canonical source of truth lives in `provider.schema.ts` because provider
 * connection testing belongs to the provider-config contract surface.
 */
export const ProviderConnectionTestArgsSchema = ConnectionTestSchema;

export type ProviderConnectionTestArgs = z.infer<typeof ProviderConnectionTestArgsSchema>;

/**
<<<<<<< HEAD
 * Legacy alias kept for compatibility with older imports.
 */
export const ProviderConnectionTestResultSchema = ConnectionTestResultSchema;
=======
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
>>>>>>> 4c47d2aa1ccf6ef1885b16104e3665fca6828162

export type ProviderConnectionTestResult = z.infer<typeof ProviderConnectionTestResultSchema>;
