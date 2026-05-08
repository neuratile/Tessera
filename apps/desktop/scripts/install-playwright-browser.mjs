import { spawn } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const currentDir = path.dirname(fileURLToPath(import.meta.url));
const desktopRoot = path.resolve(currentDir, '..');
const cliPath = path.resolve(desktopRoot, '../../node_modules/@playwright/test/cli.js');

const child = spawn(process.execPath, [cliPath, 'install', 'chromium'], {
  cwd: desktopRoot,
  stdio: 'inherit',
  env: {
    ...process.env,
    PLAYWRIGHT_BROWSERS_PATH: '0',
  },
});

child.once('exit', (code) => {
  process.exit(code ?? 1);
});
