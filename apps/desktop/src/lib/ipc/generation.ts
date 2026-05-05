import {
  type GenerateArgs,
  GenerateArgsSchema,
  type GenerateResponse,
  GenerateResponseSchema,
} from '@testing-ide/shared';

import { IpcError } from './error';
import { invokeAndParse } from './invoke';

/**
 * Trigger artifact generation. Validates `args` against `GenerateArgsSchema`
 * before sending so callers fail fast on bad input rather than waiting for
 * the backend round-trip to reject.
 */
export async function generateArtifact(args: GenerateArgs): Promise<GenerateResponse> {
  const parsed = GenerateArgsSchema.safeParse(args);
  if (!parsed.success) {
    throw new IpcError('generate_artifact', `invalid arguments: ${parsed.error.message}`);
  }
  return invokeAndParse('generate_artifact', GenerateResponseSchema, { args: parsed.data });
}
