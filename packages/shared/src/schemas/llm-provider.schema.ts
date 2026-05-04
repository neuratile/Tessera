import { z } from 'zod';

/**
 * Supported LLM provider identifiers (API + persistence boundary).
 *
 * Source of truth: `ProviderKind` in
 * `apps/desktop/src-tauri/src/providers/factory.rs`. Each Rust variant
 * carries an explicit `#[serde(rename = "...")]` and matching string
 * here keeps the discriminator stable across the IPC boundary.
 *
 * Local Ollama is `ollama` (not `ollama-local`) so the cloud variant
 * gets the explicit `-cloud` suffix while the default — local — wears
 * the bare name.
 */
export const LlmProviderIdSchema = z.union([
  z.literal('ollama'),
  z.literal('ollama-cloud'),
  z.literal('openai'),
  z.literal('openrouter'),
  z.literal('anthropic'),
]);

export type LlmProvider = z.infer<typeof LlmProviderIdSchema>;

/** Alias matching product/docs naming (`LLMProvider`). */
export type LLMProvider = LlmProvider;
