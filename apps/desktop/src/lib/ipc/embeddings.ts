import {
  type EmbeddingConfigView,
  EmbeddingConfigViewSchema,
  type EmbeddingPreset,
  EmbeddingPresetSchema,
  type IndexStatus,
  IndexStatusSchema,
  type SaveEmbeddingConfigArgs,
  SaveEmbeddingConfigArgsSchema,
  type TestEmbeddingResult,
  TestEmbeddingResultSchema,
} from '@testing-ide/shared';
import { z } from 'zod';

import { IpcError } from './error';
import { invokeAndParse } from './invoke';

const EmbeddingPresetListSchema = z.array(EmbeddingPresetSchema);

/**
 * The active embedding config, or the implicit local-Ollama default
 * (`id: null`) when nothing has been saved yet.
 */
export async function getEmbeddingConfig(): Promise<EmbeddingConfigView> {
  return invokeAndParse('get_embedding_config', EmbeddingConfigViewSchema);
}

/**
 * Save / upsert the embedding selection and mark it active. Same key
 * semantics as `saveProviderConfig`: `apiKey: undefined` preserves the
 * stored key, `apiKey: ''` clears it.
 */
export async function saveEmbeddingConfig(
  args: SaveEmbeddingConfigArgs,
): Promise<EmbeddingConfigView> {
  const parsed = SaveEmbeddingConfigArgsSchema.safeParse(args);
  if (!parsed.success) {
    throw new IpcError('save_embedding_config', `invalid arguments: ${parsed.error.message}`);
  }
  return invokeAndParse('save_embedding_config', EmbeddingConfigViewSchema, {
    args: parsed.data,
  });
}

/**
 * Probe the given (possibly unsaved) settings: embeds one string and
 * returns latency plus the model's native dimension so the UI can
 * auto-fill the dimension field.
 */
export async function testEmbeddingConnection(
  args: SaveEmbeddingConfigArgs,
): Promise<TestEmbeddingResult> {
  const parsed = SaveEmbeddingConfigArgsSchema.safeParse(args);
  if (!parsed.success) {
    throw new IpcError(
      'test_embedding_connection',
      `invalid arguments: ${parsed.error.message}`,
    );
  }
  return invokeAndParse('test_embedding_connection', TestEmbeddingResultSchema, {
    args: parsed.data,
  });
}

/** Curated model presets — single source of truth lives in Rust. */
export async function listEmbeddingPresets(): Promise<EmbeddingPreset[]> {
  return invokeAndParse('list_embedding_presets', EmbeddingPresetListSchema);
}

/**
 * Compare a project's chunk index against the active embedding config
 * (stale-index banner data source).
 */
export async function getIndexStatus(projectId: string): Promise<IndexStatus> {
  return invokeAndParse('get_index_status', IndexStatusSchema, { projectId });
}
