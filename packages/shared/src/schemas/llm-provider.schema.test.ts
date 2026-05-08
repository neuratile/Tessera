import { describe, expect, it } from 'vitest';

import {
  LlmProviderIdSchema,
  type LLMProvider,
} from './llm-provider.schema';

describe('LlmProviderIdSchema', () => {
  it('accepts every canonical provider literal', () => {
    const valid: LLMProvider[] = [
      'ollama',
      'ollama-cloud',
      'openai',
      'openrouter',
      'anthropic',
    ];
    for (const provider of valid) {
      expect(LlmProviderIdSchema.parse(provider)).toBe(provider);
    }
  });

  it('rejects legacy and unknown identifiers', () => {
    const invalid = [
      'ollama-local',
      'gpt4',
      '',
      'claude',
      'local-ai',
    ];
    for (const value of invalid) {
      expect(LlmProviderIdSchema.safeParse(value).success).toBe(false);
    }
  });
});
