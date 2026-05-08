import { mkdir, readFile, readdir, stat, writeFile } from 'node:fs/promises';
import path from 'node:path';

import type { Page } from '@playwright/test';

import type { TestingIdeE2eConfig } from '../../src/lib/e2e-bridge';

type HarnessOptions = {
  fixtureRoot: string;
  exportFilePath: string;
};

type TestingIdeWindow = Window & {
  __TESTING_IDE_E2E__?: TestingIdeE2eConfig;
};

function ensureInside(rootPath: string, candidatePath: string, label: string): string {
  const resolvedRoot = path.resolve(rootPath);
  const resolvedCandidate = path.resolve(candidatePath);
  const relative = path.relative(resolvedRoot, resolvedCandidate);

  if (relative.startsWith('..') || path.isAbsolute(relative)) {
    throw new Error(`${label} path escapes the allowed root: ${candidatePath}`);
  }

  return resolvedCandidate;
}

export async function installDesktopE2eHarness(
  page: Page,
  options: HarnessOptions,
): Promise<void> {
  const fixtureRoot = path.resolve(options.fixtureRoot);
  const exportFilePath = path.resolve(options.exportFilePath);
  const exportRoot = path.dirname(exportFilePath);

  await mkdir(exportRoot, { recursive: true });

  await page.exposeFunction('__testingIdeReadDir__', async (absolutePath: string) => {
    const safePath = ensureInside(fixtureRoot, absolutePath, 'fixture');
    const entries = await readdir(safePath, { withFileTypes: true });

    return entries.map((entry) => ({
      name: entry.name,
      isDirectory: entry.isDirectory(),
    }));
  });

  await page.exposeFunction('__testingIdeReadTextFile__', async (absolutePath: string) => {
    const safePath = ensureInside(fixtureRoot, absolutePath, 'fixture');
    return readFile(safePath, 'utf8');
  });

  await page.exposeFunction('__testingIdeStat__', async (absolutePath: string) => {
    const safePath = ensureInside(fixtureRoot, absolutePath, 'fixture');
    const metadata = await stat(safePath);

    return { size: metadata.size };
  });

  await page.exposeFunction(
    '__testingIdeWriteTextFile__',
    async (absolutePath: string, data: string) => {
      const safePath = ensureInside(exportRoot, absolutePath, 'export');
      await mkdir(path.dirname(safePath), { recursive: true });
      await writeFile(safePath, data, 'utf8');
    },
  );

  await page.addInitScript(
    ({ injectedFixtureRoot, injectedExportFilePath }) => {
      window.localStorage.setItem('testing-ide.onboarding.complete', 'true');
      const testingIdeWindow = window as TestingIdeWindow;
      testingIdeWindow.__TESTING_IDE_E2E__ = {
        enabled: true,
        fixtureRoot: injectedFixtureRoot,
        exportFilePath: injectedExportFilePath,
      };
    },
    {
      injectedFixtureRoot: fixtureRoot,
      injectedExportFilePath: exportFilePath,
    },
  );
}
