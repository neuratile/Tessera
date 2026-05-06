import { z } from 'zod';

/**
 * Locally-pulled Ollama model — mirrors the `OllamaModel` payload
 * returned by `commands::providers::list_ollama_models`.
 */
export const OllamaModelSchema = z.object({
  name: z.string(),
  sizeBytes: z.number().int().nonnegative(),
});

export type OllamaModel = z.infer<typeof OllamaModelSchema>;
