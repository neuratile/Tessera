import { z } from 'zod';

const IsoDateTimeSchema = z.string().datetime({ offset: true });

/**
 * Chunk-kind discriminator. Source of truth lives in
 * `apps/desktop/src-tauri/src/services/chunking_service.rs::ChunkKind`,
 * which serializes via serde as snake_case literals — keep these
 * variants in lock-step with the Rust enum.
 */
export const ChunkTypeSchema = z.union([
  z.literal('function'),
  z.literal('method'),
  z.literal('class'),
  z.literal('module'),
]);

export type ChunkType = z.infer<typeof ChunkTypeSchema>;

/**
 * Packed float32 embedding as a number array (API boundary; storage may use BLOB).
 */
export const EmbeddingVectorSchema = z.array(z.number().finite());

export const CodeChunkSchema = z
  .object({
    id: z.string().uuid(),
    projectId: z.string().uuid(),
    fileId: z.string().uuid(),
    chunkType: ChunkTypeSchema,
    /**
     * Identifier of the wrapped declaration. Empty for `module` kind
     * chunks (top-level imports / config blocks have no name) — matches
     * the Rust producer in `chunking_service::push_module_slice`.
     */
    name: z.string(),
    content: z.string(),
    startLine: z.number().int().positive(),
    endLine: z.number().int().positive(),
    tokenCount: z.number().int().nonnegative(),
    /**
     * True when `tokenCount` exceeds `TARGET_MAX_TOKENS` (1500) on the
     * Rust side. Consumers route oversize chunks through summarization
     * before LLM injection. See `chunking_service::TARGET_MAX_TOKENS`.
     */
    oversize: z.boolean().optional(),
    embedding: EmbeddingVectorSchema.optional(),
    embeddingDim: z.number().int().positive().optional(),
    embeddingProvider: z.string().min(1).optional(),
    embeddingModel: z.string().min(1).optional(),
    metadata: z.record(z.string(), z.unknown()).optional(),
    createdAt: IsoDateTimeSchema,
    updatedAt: IsoDateTimeSchema,
  })
  .refine((chunk) => chunk.endLine >= chunk.startLine, {
    message: 'endLine must be greater than or equal to startLine',
    path: ['endLine'],
  })
  .refine((chunk) => chunk.chunkType === 'module' || chunk.name.length > 0, {
    message: 'name must be non-empty for non-module chunk kinds',
    path: ['name'],
  });

export type CodeChunk = z.infer<typeof CodeChunkSchema>;
