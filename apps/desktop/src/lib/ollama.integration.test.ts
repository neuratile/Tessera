import { ConnectionTestResultSchema } from '@testing-ide/shared';
import { describe, expect, test } from 'vitest';
import { z } from 'zod';

import {
  buildUrl,
  fetchJson,
  isModelMatch,
  resolveIntegrationContext,
  runCargoJsonProbeTest,
} from '../../tests/support/ollama';

const ChatCompletionResponseSchema = z.object({
  choices: z
    .array(
      z.object({
        message: z.object({
          content: z.string().min(1),
        }),
      }),
    )
    .min(1),
});

const EmbeddingResponseSchema = z.object({
  data: z
    .array(
      z.object({
        embedding: z.array(z.number()).min(1),
      }),
    )
    .min(1),
});

const context = await resolveIntegrationContext({ requireEmbedding: true });
if (!context.ready) {
  process.stderr.write(`[skip] Ollama integration tests: ${context.reason}\n`);
}
const integrationTest = context.ready ? test : test.skip;

describe('Ollama integration', () => {
  integrationTest('lists models through the real provider connection flow', async () => {
    if (!context.ready) {
      return;
    }

    const result = await runCargoJsonProbeTest(
      'services::ollama_probe_test_support::tests::provider_connection_probe_emits_json',
      ConnectionTestResultSchema,
      {
        OLLAMA_PROBE_PROVIDER: 'ollama',
        OLLAMA_PROBE_BASE_URL: context.baseUrl,
        OLLAMA_PROBE_MODEL: context.chatModel.requested,
      },
    );

    expect(result.ok).toBe(true);
    expect(result.latencyMs).toBeGreaterThanOrEqual(0);
    expect(result.models.length).toBeGreaterThan(0);
    expect(result.models.some((model) => isModelMatch(context.chatModel.requested, model))).toBe(
      true,
    );
  });

  integrationTest('returns the installed model list from Ollama', async () => {
    if (!context.ready || context.embedModel === null) {
      return;
    }

    const tags = await fetchJson(
      buildUrl(
        context.baseUrl,
        '/api/tags',
      ),
      z.object({
        models: z.array(
          z.object({
            name: z.string().min(1),
          }),
        ),
      }),
    );

    expect(tags.models.length).toBeGreaterThan(0);
    expect(tags.models.some((model) => model.name === context.chatModel.installed)).toBe(true);
    expect(tags.models.some((model) => model.name === context.embedModel?.installed)).toBe(true);
  });

  integrationTest('responds to a short completion request', async () => {
    if (!context.ready) {
      return;
    }

    const response = await fetchJson(
      buildUrl(context.baseUrl, '/v1/chat/completions'),
      ChatCompletionResponseSchema,
      {
        method: 'POST',
        headers: {
          'content-type': 'application/json',
        },
        body: JSON.stringify({
          model: context.chatModel.installed,
          messages: [{ role: 'user', content: 'Reply with a short greeting.' }],
          stream: false,
          temperature: 0,
          max_tokens: 32,
        }),
      },
    );

    const content = response.choices[0]?.message.content.trim() ?? '';
    expect(content.length).toBeGreaterThan(0);
  });

  integrationTest('returns a finite embedding vector with the expected dimension', async () => {
    if (!context.ready || context.embedModel === null) {
      return;
    }

    const response = await fetchJson(
      buildUrl(context.baseUrl, '/v1/embeddings'),
      EmbeddingResponseSchema,
      {
        method: 'POST',
        headers: {
          'content-type': 'application/json',
        },
        body: JSON.stringify({
          model: context.embedModel.installed,
          input: ['Testing IDE Ollama integration smoke test.'],
        }),
      },
    );

    const vector = response.data[0]?.embedding ?? [];
    expect(vector).toHaveLength(768);
    expect(vector.every((value) => Number.isFinite(value))).toBe(true);
  });
});
