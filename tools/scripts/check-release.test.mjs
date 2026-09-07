import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import test from 'node:test';
import { checkRelease, validateRelease } from './check-release.mjs';

test('accepts a matching stable release including a zero patch', () => {
  assert.equal(validateRelease('refs/tags/v1.2.0', { package: '1.2.0', rust: '1.2.0', tauri: '1.2.0' }), '1.2.0');
});

test('rejects branches, missing refs, prereleases, partial and malformed versions', () => {
  for (const ref of [undefined, '', 'refs/heads/v1.2.3', 'v1.2.3', 'refs/tags/v1.2', 'refs/tags/v1.2.3-beta.1', 'refs/tags/v01.2.3', 'refs/tags/v1.2.3\n']) {
    assert.throws(() => validateRelease(ref, { package: '1.2.3' }), /stable refs\/tags/);
  }
});

test('rejects every mismatched or missing desktop manifest version', () => {
  for (const name of ['package', 'rust', 'tauri']) {
    for (const wrong of ['1.2.4', undefined, 123]) {
      assert.throws(() => validateRelease('refs/tags/v1.2.3', {
        package: '1.2.3', rust: '1.2.3', tauri: '1.2.3', [name]: wrong,
      }), /does not match tag/);
    }
  }
});

test('reads actual manifests, ignores dependency versions, and fails CLI on bad input', () => {
  const root = mkdtempSync(join(tmpdir(), 'tessera-release-'));
  try {
    const desktop = join(root, 'apps/desktop');
    const backend = join(desktop, 'src-tauri');
    mkdirSync(backend, { recursive: true });
    writeFileSync(join(desktop, 'package.json'), JSON.stringify({ version: '1.2.3' }));
    writeFileSync(join(backend, 'tauri.conf.json'), JSON.stringify({ version: '1.2.3' }));
    const cargoPath = join(backend, 'Cargo.toml');
    writeFileSync(cargoPath, '[package]\r\nname = "test"\r\nversion = "1.2.3"\r\n[dependencies.other]\r\nversion = "9.9.9"\r\n');
    assert.equal(checkRelease('refs/tags/v1.2.3', root), '1.2.3');
    const script = fileURLToPath(new URL('./check-release.mjs', import.meta.url));
    const child = spawnSync(process.execPath, [script, 'refs/heads/master'], { cwd: root, encoding: 'utf8' });
    assert.equal(child.status, 1);
    assert.match(child.stderr, /stable refs\/tags/);
    writeFileSync(cargoPath, '[package]\nname = "test"\n[dependencies.other]\nversion = "1.2.3"\n');
    assert.throws(() => checkRelease('refs/tags/v1.2.3', root), /Rust crate version undefined/);
    writeFileSync(join(desktop, 'package.json'), '{broken');
    assert.throws(() => checkRelease('refs/tags/v1.2.3', root), SyntaxError);
  } finally {
    // The exact directory returned by mkdtemp is the only cleanup target.
    rmSync(resolve(root), { recursive: true, force: true });
  }
});
