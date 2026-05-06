import { z } from 'zod'

export const ProjectFileSchema = z.object({
  id: z.string(),
  name: z.string(),
  path: z.string(),
  type: z.enum(['file', 'directory']),
  language: z.string().nullable(),
  content: z.string().optional()
})
// Workaround for recursive schema
export type ProjectFileType = z.infer<typeof ProjectFileSchema> & { children?: ProjectFileType[] }

export const ReviewItemSchema = z.object({
  id: z.string(),
  filePath: z.string(),
  generatedTest: z.string(),
  status: z.enum(['pending', 'approved', 'rejected', 'regenerating']),
  feedback: z.string()
})

export const AiGenerationResponseSchema = z.object({
  status: z.enum(['streaming', 'done', 'error']),
  progress: z.number().optional(),
  currentFile: z.string().optional(),
  generatedCount: z.number().optional(),
  error: z.string().optional()
})
