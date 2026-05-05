import { z } from 'zod';

/**
 * Health-check IPC response — mirrors `HealthStatus` from
 * `apps/desktop/src-tauri/src/services/health_service.rs`.
 *
 * Memory values are megabytes. `cpuCount` is the logical CPU count from
 * `sysinfo`. `dbOk` is `true` when a `SELECT 1` round-trip against the
 * managed pool succeeds.
 */
export const HealthStatusSchema = z.object({
  dbOk: z.boolean(),
  osName: z.string(),
  osVersion: z.string(),
  totalMemoryMb: z.number().int().nonnegative(),
  availableMemoryMb: z.number().int().nonnegative(),
  cpuCount: z.number().int().nonnegative(),
});

export type HealthStatus = z.infer<typeof HealthStatusSchema>;
