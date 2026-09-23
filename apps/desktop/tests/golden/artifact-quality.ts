import type { TestCase } from '@testing-ide/shared';

// This is a conservative *textual design* check, not test execution or code coverage.
// Each hit requires both a scenario-specific action/input and an expected observation
// in the same case. Keep these expectations aligned with fixtures/express-api/src.
const scenarios = [
  {
    id: 'login-success',
    input: /\blogin\b/i,
    detail: /\b(valid|correct|success)\b/i,
    observation: /(?=.*\b200\b)(?=.*\b(?:sessionToken|session token)\b)/i,
    excluded: /\b(invalid|wrong|incorrect|unknown|missing|empty|blank)\b/i,
  },
  {
    id: 'login-missing-fields',
    input: /\blogin\b/i,
    detail: /\b(missing|empty|blank|required|omit(?:ted)?|no password|no email)\b/i,
    observation: /\b400\b/i,
  },
  {
    id: 'login-invalid-credentials',
    input: /\blogin\b/i,
    detail: /\b(invalid|wrong|incorrect|unknown)\b/i,
    observation: /\b400\b/i,
    excluded: /\b(missing|empty|blank|required|omit(?:ted)?|no password|no email)\b/i,
  },
  {
    id: 'logout-known-token',
    input: /\blogout\b/i,
    detail: /\b(valid|existing|active|known|current)\b/i,
    observation: /\b(204|no content)\b/i,
    excluded: /\b(invalid|unknown|nonexistent|non-existent|expired|revoked|missing|empty)\b/i,
  },
  {
    id: 'logout-unknown-token',
    input: /\blogout\b/i,
    detail: /\b(invalid|unknown|nonexistent|non-existent|expired|revoked|missing|empty)\b/i,
    observation: /\b(404|not found)\b/i,
  },
] as const;

export function scoreExpressAuthCases(cases: TestCase['cases']) {
  const matches = scenarios.map((scenario) => ({
    scenario: scenario.id,
    caseIds: cases
      .filter((testCase) => testCase.steps.some((step) => {
        const inputs = [testCase.title, testCase.testData ?? '', ...(testCase.preconditions ?? []), step.action].join(' ');
        return scenario.input.test(step.action)
          && scenario.detail.test(inputs)
          && (!('excluded' in scenario) || !scenario.excluded.test(inputs))
          && scenario.observation.test(step.expectedResult);
      }))
      .map((testCase) => testCase.id),
  }));

  return {
    covered: matches.filter(({ caseIds }) => caseIds.length > 0).length,
    total: scenarios.length,
    matches,
  };
}
