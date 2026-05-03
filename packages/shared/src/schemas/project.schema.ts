import { z } from 'zod';

const IsoDateTimeSchema = z.string().datetime({ offset: true });

export const ProjectStatusSchema = z.union([
  z.literal('uploading'),
  z.literal('analyzing'),
  z.literal('ready'),
  z.literal('error'),
]);

export type ProjectStatus = z.infer<typeof ProjectStatusSchema>;

/**
 * Language breakdown keyed by language identifier (e.g. `typescript` → file count).
 */
export const LanguageBreakdownSchema = z.record(z.string(), z.number().int().nonnegative());

export const ProjectSchema = z.object({
  id: z.string().uuid(),
  userId: z.string().uuid(),
  name: z.string().min(1),
  fileCount: z.number().int().nonnegative(),
  totalSize: z.number().int().nonnegative(),
  status: ProjectStatusSchema,
  languageBreakdown: LanguageBreakdownSchema,
  createdAt: IsoDateTimeSchema,
  updatedAt: IsoDateTimeSchema,
});

export type Project = z.infer<typeof ProjectSchema>;
