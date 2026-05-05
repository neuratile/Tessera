import { z } from 'zod';

/**
 * Project schema — mirrors `ProjectResponse` from
 * `apps/desktop/src-tauri/src/commands/projects.rs`.
 *
 * Phase 6 IPC layer omits `userId` (single-user desktop app, no auth)
 * and exposes `rootPath` + `totalSizeBytes` directly. Timestamps come
 * back as RFC 3339 strings via `Utc::now().to_rfc3339()`.
 */

const IsoDateTimeSchema = z.string().datetime({ offset: true });

export const ProjectStatusSchema = z.union([
  z.literal('pending'),
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
  name: z.string().min(1),
  rootPath: z.string().min(1),
  fileCount: z.number().int().nonnegative(),
  totalSizeBytes: z.number().int().nonnegative(),
  status: ProjectStatusSchema,
  languageBreakdown: LanguageBreakdownSchema,
  createdAt: IsoDateTimeSchema,
  updatedAt: IsoDateTimeSchema,
});

export type Project = z.infer<typeof ProjectSchema>;
