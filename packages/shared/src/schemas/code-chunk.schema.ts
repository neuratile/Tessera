import { z } from 'zod';

const IsoDateTimeSchema = z.string().datetime({ offset: true });

export const ChunkTypeSchema = z.union([
  z.literal('function'),
  z.literal('class'),
  z.literal('module'),
  z.literal('block'),
  z.literal('other'),
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
    name: z.string().min(1),
    content: z.string(),
    startLine: z.number().int().positive(),
    endLine: z.number().int().positive(),
    tokenCount: z.number().int().nonnegative(),
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
  });

export type CodeChunk = z.infer<typeof CodeChunkSchema>;
