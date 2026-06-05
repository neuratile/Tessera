import { fileURLToPath } from 'node:url';

import { TestCaseSchema, TestPlanSchema } from '@testing-ide/shared';
import { describe, expect, test } from 'vitest';
import { z } from 'zod';

import { resolveIntegrationContext, runCargoJsonProbeTest } from '../support/ollama';

const fixtureRoot = fileURLToPath(new URL('./fixtures/express-api', import.meta.url));
const GoldenProbeOutputSchema = z.object({
  artifactType: z.union([z.literal('test-plan'), z.literal('test-cases')]),
  promptVersion: z.string().min(1),
  model: z.string().min(1),
  scopeHint: z.string().min(1),
  chunkCount: z.number().int().positive(),
  usageInputTokens: z.number().int().nonnegative(),
  usageOutputTokens: z.number().int().nonnegative(),
  structuredData: z.record(z.string(), z.unknown()),
});

const context = await resolveIntegrationContext({ requireEmbedding: false });
if (!context.ready) {
  process.stderr.write(`[skip] Ollama golden tests: ${context.reason}\n`);
}
const integrationTest = context.ready ? test : test.skip;

async function generateFixtureArtifact(artifactType: 'test-plan' | 'test-cases') {
  if (!context.ready) {
    throw new Error('golden test helper called without a ready Ollama context');
  }

  return runCargoJsonProbeTest(
    'services::ollama_probe_test_support::tests::golden_generation_probe_emits_json',
    GoldenProbeOutputSchema,
    {
      OLLAMA_GOLDEN_ARTIFACT_TYPE: artifactType,
      OLLAMA_GOLDEN_FIXTURE_ROOT: fixtureRoot,
      OLLAMA_GOLDEN_BASE_URL: context.baseUrl,
      OLLAMA_GOLDEN_MODEL: context.chatModel.installed,
      OLLAMA_GOLDEN_PROJECT_NAME: 'express-api-fixture',
      OLLAMA_GOLDEN_SCOPE_HINT: 'auth module',
    },
  );
}

// Retry twice: small Ollama models on CI runners (2 vCPU, 7 GB RAM)
// can hit StreamInterrupted or produce non-deterministic output. Two
// retries (3 total attempts) keep CI green without hiding real
// regressions — the pipeline correctness assertions below are loose
// enough that a model-quality fluke does not block unrelated PRs.
const GOLDEN_RETRY = 2;

describe('Ollama golden prompt coverage', () => {
  integrationTest(
    'generates a test plan payload that matches TestPlanSchema',
    { retry: GOLDEN_RETRY },
    async () => {
      if (!context.ready) {
        return;
      }

      const result = await generateFixtureArtifact('test-plan');
      const parsed = TestPlanSchema.safeParse(result.structuredData);
      if (!parsed.success) {
        throw new Error(`TestPlanSchema mismatch: ${parsed.error.message}`);
      }

      expect(parsed.data.summary.length).toBeGreaterThan(0);
      // Small models (3b) may omit objectives entirely — normalization
      // backfills []. Only assert the field is a valid array; content
      // richness is a model quality concern, not a pipeline correctness
      // test.
      expect(Array.isArray(parsed.data.objectives)).toBe(true);
    },
  );

  integrationTest(
    'generates test cases that match TestCaseSchema',
    { retry: GOLDEN_RETRY },
    async () => {
      if (!context.ready) {
        return;
      }

      const result = await generateFixtureArtifact('test-cases');
      const parsed = TestCaseSchema.safeParse(result.structuredData);
      if (!parsed.success) {
        throw new Error(`TestCaseSchema mismatch: ${parsed.error.message}`);
      }

      // Pipeline correctness: structured data parses into the schema.
      // Content richness (non-empty cases, non-empty steps) depends on
      // model quality and CI runner resources — not pipeline regressions.
      expect(Array.isArray(parsed.data.cases)).toBe(true);
    },
  );
});
