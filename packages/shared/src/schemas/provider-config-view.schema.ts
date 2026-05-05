import { z } from 'zod';

import { LlmProviderIdSchema } from './llm-provider.schema';

/**
 * Frontend-safe view of a stored provider config — mirrors
 * `ProviderConfigView` from
 * `apps/desktop/src-tauri/src/services/provider_config_service.rs`.
 *
 * The plaintext API key is **never** sent over IPC; `hasApiKey` is the
 * boolean signal the UI uses to render "Key set / Key missing" badges.
 */
export const ProviderConfigViewSchema = z.object({
  id: z.string().uuid(),
  provider: LlmProviderIdSchema,
  hasApiKey: z.boolean(),
  baseUrl: z.string().nullable().optional(),
  defaultModel: z.string().nullable().optional(),
  isActive: z.boolean(),
});

export type ProviderConfigView = z.infer<typeof ProviderConfigViewSchema>;

/**
 * Arguments accepted by `save_provider_config` — mirrors
 * `SaveProviderArgs` in `commands/providers.rs`.
 */
export const SaveProviderArgsSchema = z.object({
  provider: LlmProviderIdSchema,
  apiKey: z.string().optional(),
  baseUrl: z.string().optional(),
  defaultModel: z.string().optional(),
  isActive: z.boolean().optional(),
});

export type SaveProviderArgs = z.infer<typeof SaveProviderArgsSchema>;
