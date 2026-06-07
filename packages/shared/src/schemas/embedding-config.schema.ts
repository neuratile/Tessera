import { z } from 'zod';

/**
 * Embedding provider identifiers - mirrors `EmbeddingProviderKind` in
 * `apps/desktop/src-tauri/src/providers/factory.rs` (serde rename
 * strings must match exactly, rules.md §12.3.1).
 *
 * Deliberately separate from `LlmProviderIdSchema`: the LLM catalog
 * (anthropic, openrouter) and the embedding catalog (huggingface)
 * diverge.
 */
export const EmbeddingProviderIdSchema = z.union([
  z.literal('ollama'),
  z.literal('ollama-cloud'),
  z.literal('openai'),
  z.literal('gemini'),
  z.literal('huggingface'),
]);

export type EmbeddingProviderId = z.infer<typeof EmbeddingProviderIdSchema>;

/**
 * Frontend-safe view of the active embedding config - mirrors
 * `EmbeddingConfigView` in `services/embedding_config_service.rs`.
 *
 * `id` is `null` when the user has no stored row yet and the implicit
 * local-Ollama default applies. The plaintext API key never crosses
 * IPC; `hasApiKey` drives the "Key set / Key missing" badge.
 */
export const EmbeddingConfigViewSchema = z.object({
  id: z.string().uuid().nullable(),
  provider: EmbeddingProviderIdSchema,
  model: z.string().min(1),
  dimension: z.number().int().positive(),
  baseUrl: z.string().nullable().optional(),
  hasApiKey: z.boolean(),
  isActive: z.boolean(),
});

export type EmbeddingConfigView = z.infer<typeof EmbeddingConfigViewSchema>;

/**
 * Arguments accepted by `save_embedding_config` and
 * `test_embedding_connection` - mirrors `SaveEmbeddingConfigArgs` in
 * `commands/embeddings.rs`.
 *
 * Save semantics (same contract as `SaveProviderArgsSchema`):
 * - `apiKey: undefined` preserves any previously stored encrypted key.
 * - `apiKey: ''` clears the stored key.
 *
 * Dimension bounds mirror `embedding_config_service::MAX_DIMENSION`.
 */
export const SaveEmbeddingConfigArgsSchema = z.object({
  provider: EmbeddingProviderIdSchema,
  model: z.string().min(1),
  dimension: z.number().int().min(1).max(8192),
  baseUrl: z.string().optional(),
  apiKey: z.string().optional(),
});

export type SaveEmbeddingConfigArgs = z.infer<typeof SaveEmbeddingConfigArgsSchema>;

/**
 * Result of the Settings connection test - mirrors
 * `TestEmbeddingResult` in `services/embedding_config_service.rs`.
 * `detectedDimension` auto-fills the dimension field in the UI.
 */
export const TestEmbeddingResultSchema = z.object({
  latencyMs: z.number().nonnegative(),
  detectedDimension: z.number().int().positive(),
});

export type TestEmbeddingResult = z.infer<typeof TestEmbeddingResultSchema>;

/**
 * One curated provider/model preset - mirrors `EmbeddingPreset` in
 * `providers/embeddings/presets.rs`.
 */
export const EmbeddingPresetSchema = z.object({
  provider: EmbeddingProviderIdSchema,
  model: z.string().min(1),
  dimension: z.number().int().positive(),
  isDefault: z.boolean(),
});

export type EmbeddingPreset = z.infer<typeof EmbeddingPresetSchema>;

/**
 * `(provider, model, dimension)` triple identifying one embedding
 * space - mirrors `EmbeddingSignatureView` in
 * `services/embedding_config_service.rs`.
 *
 * `provider` is a plain string (not the id union): for indexed chunks
 * it is the raw stored composite (`"ollama-nomic-embed-text"`), and
 * legacy rows may carry identifiers from removed providers - status
 * display must not explode on them.
 */
export const EmbeddingSignatureSchema = z.object({
  provider: z.string(),
  model: z.string(),
  dimension: z.number().int().nonnegative(),
});

export type EmbeddingSignature = z.infer<typeof EmbeddingSignatureSchema>;

/**
 * Stale-index status for one project - mirrors `IndexStatus` in
 * `services/embedding_config_service.rs`. `indexedWith` is `null` for
 * never-indexed projects (and `isStale` is then `false`).
 */
export const IndexStatusSchema = z.object({
  projectId: z.string(),
  embeddedChunks: z.number().int().nonnegative(),
  indexedWith: EmbeddingSignatureSchema.nullable(),
  activeConfig: EmbeddingSignatureSchema,
  isStale: z.boolean(),
});

export type IndexStatus = z.infer<typeof IndexStatusSchema>;
