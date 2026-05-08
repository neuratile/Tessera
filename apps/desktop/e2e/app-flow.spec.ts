import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { expect, test } from '@playwright/test';

import { installDesktopE2eHarness } from './support/desktop-e2e';

const currentDir = path.dirname(fileURLToPath(import.meta.url));
const fixtureRoot = path.resolve(currentDir, '../tests/golden/fixtures/express-api');

test.describe('desktop app flow', () => {
  test('should upload, analyze, generate, approve, and export a markdown artifact', async ({
    page,
  }, testInfo) => {
    const exportFilePath = testInfo.outputPath('exports/test-plan-express-api.md');

    await installDesktopE2eHarness(page, {
      fixtureRoot,
      exportFilePath,
    });

    await page.goto('/');

    await page.getByRole('button', { name: 'Open folder' }).click();
    await expect(page.getByText('src')).toBeVisible();

    await page.getByTestId('analyze-project').click();
    await expect(page.getByTestId('project-status')).toHaveText('ready');
    await expect(page.getByTestId('analysis-status')).toHaveText('9 chunks');

    const testPlanButton = page.getByRole('button', { name: 'Test plan' });
    await expect(testPlanButton).toBeEnabled();
    await testPlanButton.click();

    await expect(page.getByText('Test Plan - express-api')).toBeVisible();

    await page.getByRole('button', { name: 'Open Test Plan - express-api' }).click();
    const drawer = page.getByRole('dialog', { name: 'Review Test Plan - express-api' });
    await expect(drawer).toBeVisible();
    await expect(drawer.getByText('Covers the express-api auth and health flows.')).toBeVisible();

    await drawer.getByRole('button', { name: 'Approve' }).click();
    await expect(drawer.getByText('Approved')).toBeVisible();

    await drawer.getByRole('button', { name: 'Export markdown' }).click();
    await expect
      .poll(async () => {
        try {
          return await readFile(exportFilePath, 'utf8');
        } catch {
          return '';
        }
      })
      .toContain('# Test Plan');

    const markdown = await readFile(exportFilePath, 'utf8');
    expect(markdown).toContain('Verify login and logout behavior.');
    expect(markdown).toContain('Confirm health endpoint availability.');
  });
});
