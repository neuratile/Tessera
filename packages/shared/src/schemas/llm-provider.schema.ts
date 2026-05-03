import { z } from 'zod';

/**
 * Supported LLM provider identifiers (API + persistence boundary).
 */
export const LlmProviderIdSchema = z.union([
  z.literal('openai'),
  z.literal('anthropic'),
  z.literal('openrouter'),
  z.literal('ollama-cloud'),
  z.literal('ollama-local'),
]);

export type LlmProvider = z.infer<typeof LlmProviderIdSchema>;

/** Alias matching product/docs naming (`LLMProvider`). */
export type LLMProvider = LlmProvider;
