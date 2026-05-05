import { z } from 'zod';

import { invokeAndParse, invokeString } from './invoke';

const InitDbResponseSchema = z.object({
  dbPath: z.string(),
  ok: z.boolean(),
});

export type InitDbResponse = z.infer<typeof InitDbResponseSchema>;

/** Phase 1 smoke command — confirms DB pool reachability. */
export async function initDb(): Promise<InitDbResponse> {
  return invokeAndParse('init_db', InitDbResponseSchema);
}

/** Phase 1 sanity command — round-trips a string through the IPC bridge. */
export async function greet(name: string): Promise<string> {
  return invokeString('greet', { name });
}
