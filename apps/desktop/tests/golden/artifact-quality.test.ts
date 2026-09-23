import type { TestCase } from '@testing-ide/shared';
import { describe, expect, test } from 'vitest';

import { scoreExpressAuthCases } from './artifact-quality';

function example(id: string, title: string, action: string, expectedResult: string): TestCase['cases'][number] {
  return { id, title, type: 'positive', priority: 'p1', steps: [{ action, expectedResult }] };
}

describe('express auth artifact design score', () => {
  test('counts independently specified actions and expected observations', () => {
    const result = scoreExpressAuthCases([
      example('TC-1', 'valid login', 'POST login with qa@example.com and correct password', '200 with sessionToken and public user'),
      example('TC-2', 'missing email login', 'POST login with empty email', '400 required'),
      example('TC-3', 'invalid login', 'POST login with wrong password', '400 invalid credentials'),
      example('TC-4', 'logout', 'POST logout using current session token', '204 No Content'),
      example('TC-5', 'unknown logout', 'POST logout with unknown token', '404 not found'),
    ]);
    expect(result.covered).toBe(5);
    expect(result.total).toBe(5);
    expect(result.matches.map(({ caseIds }) => caseIds)).toEqual([['TC-1'], ['TC-2'], ['TC-3'], ['TC-4'], ['TC-5']]);
  });

  test('does not infer assertions from a title or from a different case', () => {
    const result = scoreExpressAuthCases([
      example('TC-1', 'valid login returns 200 and sessionToken', 'POST login with correct password', 'Response is successful'),
      example('TC-2', 'other request', 'GET health endpoint', '200 and sessionToken'),
      example('TC-3', 'unknown logout', 'POST logout with unknown token', '204 No Content'),
      example('TC-4', 'valid login', 'POST login with correct password', '200 OK without a specified token'),
      example('TC-5', 'missing email login', 'POST login with empty email', 'email is required'),
      example('TC-6', 'valid login', 'GET health with valid token', '200 with sessionToken'),
    ]);
    expect(result.covered).toBe(0);
  });

  test('does not combine an action with a different step’s expected result', () => {
    const mixed = example('TC-mixed', 'login', 'POST login with empty email', '200 with sessionToken');
    mixed.steps.push({ action: 'POST login with correct password', expectedResult: '400 required' });
    expect(scoreExpressAuthCases([mixed]).covered).toBe(0);
  });

  test('does not count contradictory positive login or known-token logout cases', () => {
    const result = scoreExpressAuthCases([
      example('TC-bad-login', 'valid login with wrong password', 'POST login with wrong password for qa@example.com', '200 with sessionToken'),
      example('TC-bad-logout', 'logout with unknown token', 'POST logout with unknown session token', '204 No Content'),
    ]);
    expect(result.covered).toBe(0);
  });

  test('does not double count missing fields as invalid credentials', () => {
    const result = scoreExpressAuthCases([
      example('TC-empty', 'invalid login with missing email', 'POST login with empty email', '400 required'),
    ]);
    expect(result.matches.find(({ scenario }) => scenario === 'login-missing-fields')?.caseIds).toEqual(['TC-empty']);
    expect(result.matches.find(({ scenario }) => scenario === 'login-invalid-credentials')?.caseIds).toEqual([]);
  });

  test('keeps a wrong-password case even when preconditions mention non-empty fields', () => {
    const invalid = example('TC-wrong', 'invalid login', 'POST login with wrong password', '400 invalid credentials');
    invalid.preconditions = ['The email field must be non-empty'];
    expect(scoreExpressAuthCases([invalid]).matches.find(({ scenario }) => scenario === 'login-invalid-credentials')?.caseIds).toEqual(['TC-wrong']);
  });

  test('a 400 alone does not distinguish required fields from invalid credentials', () => {
    expect(scoreExpressAuthCases([
      example('TC-ambiguous', 'invalid login with empty email', 'POST login with empty email', '400 response'),
    ]).covered).toBe(0);
  });

  test('empty generated cases cannot be counted as coverage', () => {
    expect(scoreExpressAuthCases([]).covered).toBe(0);
  });
});
