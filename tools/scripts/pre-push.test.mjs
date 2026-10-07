import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { existsSync, mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { delimiter, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

const script = fileURLToPath(new URL('./pre-push.sh', import.meta.url)).replaceAll('\\', '/');

test('pre-push propagates every Rust gate failure instead of accepting a later success', () => {
  const root = mkdtempSync(join(tmpdir(), 'tessera-pre-push-'));
  const bin = join(root, 'bin');
  const trace = join(root, 'rust-calls');
  const backend = join(root, 'apps/desktop/src-tauri');
  try {
    mkdirSync(bin);
    mkdirSync(join(root, 'tools/scripts'), { recursive: true });
    mkdirSync(backend, { recursive: true });
    const stub = (path, body) => writeFileSync(path, '#!/usr/bin/env bash\n' + body, { mode: 0o755 });
    stub(join(bin, 'git'), 'printf "%s\\n" "$TESSERA_GUARD_ROOT"\n');
    stub(join(bin, 'pnpm'), 'exit 0\n');
    stub(join(root, 'tools/scripts/pre-push-no-markers.sh'), 'exit 0\n');
    stub(join(bin, 'rustc'), `printf 'rustc\\n' >> "$TESSERA_GUARD_TRACE"
if [[ "$TESSERA_GUARD_FAIL" == rustc ]]; then exit 17; fi
`);
    stub(join(bin, 'cargo'), `printf '%s\\n' "$*" >> "$TESSERA_GUARD_TRACE"
case "$TESSERA_GUARD_FAIL" in
  clippy) [[ "$1" == clippy ]] && exit 17 ;;
  library) [[ "$1" == test && "$*" == *--lib* ]] && exit 17 ;;
  capture) [[ "$1" == test && "$*" == *--test* ]] && exit 17 ;;
esac
exit 0
`);
    for (const [failure, calls] of [['none', 4], ['rustc', 1], ['clippy', 2], ['library', 3], ['capture', 4], ['directory', 0]]) {
      rmSync(trace, { force: true });
      if (failure === 'directory') rmSync(backend, { recursive: true });
      const result = spawnSync('bash', [script], {
        cwd: root,
        encoding: 'utf8',
        env: {
          ...process.env,
          PATH: bin + delimiter + process.env.PATH,
          TESSERA_GUARD_ROOT: root.replaceAll('\\', '/'),
          TESSERA_GUARD_TRACE: trace.replaceAll('\\', '/'),
          TESSERA_GUARD_FAIL: failure,
        },
      });
      const succeeded = failure === 'none';
      assert.equal(result.status === 0, succeeded, `${failure}: ${result.stdout} ${result.stderr}`);
      assert.equal(result.stdout.includes('All local gates passed'), succeeded);
      if (!succeeded) assert.match(result.stderr, /Rust checks failed/);
      const recorded = existsSync(trace) ? readFileSync(trace, 'utf8').trim().split('\n') : [];
      assert.equal(recorded.length, calls, failure);
    }
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
