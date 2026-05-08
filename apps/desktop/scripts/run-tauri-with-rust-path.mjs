/**
 * Ensures Cargo is on PATH, then invokes the bundled `@tauri-apps/cli`.
 *
 * Many Windows terminals start without `%USERPROFILE%\.cargo\bin` (e.g. Cursor opened
 * before Rust was installed). Tauri shells out to `cargo metadata`, which fails with
 * "program not found" when `cargo` is missing from PATH even if rustup installed it.
 */
import { spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { dirname, delimiter, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import process from 'node:process';

const __dirname = dirname(fileURLToPath(import.meta.url));
const desktopRoot = dirname(__dirname);

const cargoBinParent = (() => {
  if (process.platform === 'win32') {
    return process.env.USERPROFILE ? join(process.env.USERPROFILE, '.cargo', 'bin') : '';
  }
  return process.env.HOME ? join(process.env.HOME, '.cargo', 'bin') : '';
})();

/** @returns {Record<string, string | undefined>} */
function withCargoBinPrepended(env) {
  const out = { ...env };
  if (!cargoBinParent || !existsSync(cargoBinParent)) {
    return out;
  }
  const sep = delimiter;
  /** @type {(s: string) => string[]} */
  const split = (s) => s.split(sep).filter(Boolean);
  const isWin = process.platform === 'win32';
  const current = isWin ? (out.Path ?? out.PATH ?? '') : (out.PATH ?? '');
  const merged = split(current);
  if (merged.includes(cargoBinParent)) {
    return out;
  }
  const next = `${cargoBinParent}${sep}${merged.join(sep)}`;
  if (isWin) {
    out.Path = next;
    out.PATH = next;
  } else {
    out.PATH = next;
  }
  return out;
}

const cargoExe =
  cargoBinParent && existsSync(join(cargoBinParent, process.platform === 'win32' ? 'cargo.exe' : 'cargo'));
if (!cargoExe) {
  console.error(
    [
      '[desktop] Cargo was not found. Expected:',
      cargoBinParent || '(unknown HOME/USERPROFILE)',
      '',
      'Install Rust from https://rustup.rs then fully quit and restart Cursor',
      '(or merge User PATH manually so .cargo\\\\bin appears).',
    ].join('\n'),
  );
  process.exit(1);
}

const tauriCliCandidates = [
  join(desktopRoot, 'node_modules', '@tauri-apps', 'cli', 'tauri.js'),
  resolve(desktopRoot, '../../node_modules/@tauri-apps/cli/tauri.js'),
];
const tauriCli = tauriCliCandidates.find((candidate) => existsSync(candidate));

if (!tauriCli) {
  console.error('[desktop] Missing node_modules/@tauri-apps/cli. Run: npm install');
  process.exit(1);
}

const env = withCargoBinPrepended(process.env);
const result = spawnSync(process.execPath, [tauriCli, ...process.argv.slice(2)], {
  cwd: desktopRoot,
  env,
  stdio: 'inherit',
});

process.exit(result.status === null ? 1 : result.status);
