import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

export function validateRelease(ref, versions) {
  const match = /^refs\/tags\/v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.exec(ref ?? '');
  if (!match) throw new Error('Release requires a stable refs/tags/vMAJOR.MINOR.PATCH ref.');
  const expected = match.slice(1).join('.');
  for (const [name, actual] of Object.entries(versions)) {
    if (actual !== expected) {
      throw new Error(`${name} version ${JSON.stringify(actual)} does not match tag ${expected}.`);
    }
  }
  return expected;
}

export function checkRelease(ref, root = process.cwd()) {
  const json = (path) => JSON.parse(readFileSync(resolve(root, path), 'utf8'));
  const cargo = readFileSync(resolve(root, 'apps/desktop/src-tauri/Cargo.toml'), 'utf8');
  const packageSection = cargo.match(/^\[package\]\s*\r?\n([\s\S]*?)(?=^\[|$(?![\s\S]))/m)?.[1];
  const rustVersion = packageSection?.match(/^version\s*=\s*"([^"]+)"\s*$/m)?.[1];
  return validateRelease(ref, {
    'Desktop package': json('apps/desktop/package.json').version,
    'Rust crate': rustVersion,
    'Tauri config': json('apps/desktop/src-tauri/tauri.conf.json').version,
  });
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    const version = checkRelease(process.argv[2]);
    process.stdout.write(`Release preflight passed for v${version}.\n`);
  } catch (error) {
    process.stderr.write(`${error.message}\n`);
    process.exitCode = 1;
  }
}
