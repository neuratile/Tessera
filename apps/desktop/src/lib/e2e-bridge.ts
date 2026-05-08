export type E2eDirectoryEntry = {
  name: string;
  isDirectory: boolean;
};

export type E2eStat = {
  size: number;
};

export type TestingIdeE2eConfig = {
  enabled: true;
  fixtureRoot: string;
  exportFilePath: string;
};

type TestingIdeWindow = Window & {
  __TESTING_IDE_E2E__?: TestingIdeE2eConfig;
  __testingIdeReadDir__?: (absolutePath: string) => Promise<E2eDirectoryEntry[]>;
  __testingIdeReadTextFile__?: (absolutePath: string) => Promise<string>;
  __testingIdeStat__?: (absolutePath: string) => Promise<E2eStat>;
  __testingIdeWriteTextFile__?: (absolutePath: string, data: string) => Promise<void>;
};

function getTestingIdeWindow(): TestingIdeWindow | null {
  if (typeof window === 'undefined') {
    return null;
  }

  return window;
}

export function getTestingIdeE2eConfig(): TestingIdeE2eConfig | null {
  const testingIdeWindow = getTestingIdeWindow();
  if (testingIdeWindow?.__TESTING_IDE_E2E__?.enabled !== true) {
    return null;
  }

  return testingIdeWindow.__TESTING_IDE_E2E__;
}

export function isTestingIdeE2eEnabled(): boolean {
  return getTestingIdeE2eConfig() !== null;
}

export function getTestingIdeReadDir() {
  return getTestingIdeWindow()?.__testingIdeReadDir__ ?? null;
}

export function getTestingIdeReadTextFile() {
  return getTestingIdeWindow()?.__testingIdeReadTextFile__ ?? null;
}

export function getTestingIdeStat() {
  return getTestingIdeWindow()?.__testingIdeStat__ ?? null;
}

export function getTestingIdeWriteTextFile() {
  return getTestingIdeWindow()?.__testingIdeWriteTextFile__ ?? null;
}
