import { z } from 'zod';

import { LlmProviderIdSchema } from './llm-provider.schema';

const OptionalUrlSchema = z.union([z.string().url(), z.literal('')]);

/**
 * User-facing provider configuration (forms + API). Secrets may be redacted server-side.
 */
export const ProviderConfigSchema = z.discriminatedUnion('provider', [
  z.object({
    provider: z.literal('openai'),
    apiKey: z.string().optional(),
    baseUrl: OptionalUrlSchema.optional(),
    defaultModel: z.string().min(1),
    isActive: z.boolean().optional(),
  }),
  z.object({
    provider: z.literal('anthropic'),
    apiKey: z.string().optional(),
    baseUrl: OptionalUrlSchema.optional(),
    defaultModel: z.string().min(1),
    isActive: z.boolean().optional(),
  }),
  z.object({
    provider: z.literal('openrouter'),
    apiKey: z.string().optional(),
    baseUrl: OptionalUrlSchema.optional(),
    defaultModel: z.string().min(1),
    isActive: z.boolean().optional(),
  }),
  z.object({
    provider: z.literal('ollama-cloud'),
    apiKey: z.string().optional(),
    baseUrl: OptionalUrlSchema.optional(),
    defaultModel: z.string().min(1),
    isActive: z.boolean().optional(),
  }),
  z.object({
    provider: z.literal('ollama-local'),
    apiKey: z.string().optional(),
    baseUrl: OptionalUrlSchema.optional(),
    defaultModel: z.string().min(1),
    isActive: z.boolean().optional(),
  }),
]);

export type ProviderConfig = z.infer<typeof ProviderConfigSchema>;

/**
 * Request body for "test provider connection" (API boundary).
 */
export const ConnectionTestSchema = z.object({
  provider: LlmProviderIdSchema,
  apiKey: z.string().optional(),
  baseUrl: OptionalUrlSchema.optional(),
  defaultModel: z.string().min(1).optional(),
});

export type ConnectionTestInput = z.infer<typeof ConnectionTestSchema>;

/**
 * Response body for connection test.
 */
export const ConnectionTestResultSchema = z.object({
  ok: z.boolean(),
  message: z.string().optional(),
  latencyMs: z.number().nonnegative().optional(),
});

export type ConnectionTestResult = z.infer<typeof ConnectionTestResultSchema>;
