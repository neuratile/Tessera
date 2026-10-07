import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

const script = fileURLToPath(new URL('./read-rust-toolchain.sh', import.meta.url)).replaceAll('\\', '/');

test('CI reads the repository Rust pin', () => {
  const result = spawnSync('bash', [script], { encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /^\d+\.\d+\.\d+\n$/);
});

test('toolchain reader accepts exact versions and rejects floating, missing, duplicate or injected pins', () => {
  const cwd = mkdtempSync(join(tmpdir(), 'tessera-rust-pin-'));
  try {
    const valid = '[toolchain]\nchannel = "1.99.0"\n';
    for (const [contents, accepted] of [
      [valid, true],
      ['[toolchain]\nchannel = "stable"\n', false],
      ['[toolchain]\n', false],
      [valid + 'channel = "1.98.1"\n', false],
      ['channel = "1.99.0; echo unsafe"\n', false],
    ]) {
      writeFileSync(join(cwd, 'rust-toolchain.toml'), contents);
      const result = spawnSync('bash', [script], { cwd, encoding: 'utf8' });
      assert.equal(result.status === 0, accepted, result.stderr);
      if (accepted) assert.equal(result.stdout, '1.99.0\n');
      else assert.equal(result.stdout, '');
    }
    rmSync(join(cwd, 'rust-toolchain.toml'));
    assert.notEqual(spawnSync('bash', [script], { cwd }).status, 0);
  } finally {
    rmSync(cwd, { recursive: true, force: true });
  }
});
