import assert from 'node:assert/strict';
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, relative, resolve, sep } from 'node:path';
import { spawnSync } from 'node:child_process';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import { checkoutTotal as baselineTotal } from './baseline/src/checkout.mjs';
import { checkoutTotal as regressionTotal } from './regression/src/checkout.mjs';
import { checkoutTotal as cleanTotal } from './clean/src/checkout.mjs';

const fixtureRoot = dirname(fileURLToPath(import.meta.url));
const manifest = JSON.parse(readFileSync(join(fixtureRoot, 'manifest.json'), 'utf8'));

test('the seeded change accepts negative quantities while baseline and control reject them', () => {
  const finding = manifest.scenarios[0].expectedFindings[0];
  assert.throws(() => baselineTotal(finding.failingInput.quantity), RangeError);
  assert.equal(regressionTotal(finding.failingInput.quantity), finding.observedFixtureBehavior.totalCents);
  assert.throws(() => cleanTotal(finding.failingInput.quantity), RangeError);
  assert.deepEqual(manifest.scenarios[1].expectedFindings, []);
});

test('finding location identifies the changed guard in the materialized source', () => {
  const finding = manifest.scenarios[0].expectedFindings[0];
  const baseline = readFileSync(join(fixtureRoot, manifest.baselineSource), 'utf8').split('\n');
  const regression = readFileSync(join(fixtureRoot, manifest.scenarios[0].changedSource), 'utf8').split('\n');
  assert.equal(finding.source.path, manifest.stagedPath);
  assert.equal(finding.source.side, 'new');
  assert.equal(finding.source.lineStart, finding.source.lineEnd);
  assert.match(baseline[finding.source.lineStart - 1], /quantity < 1/);
  assert.match(regression[finding.source.lineStart - 1], /quantity === 0/);
  assert.equal(finding.evidenceState, 'suspected');
});

const cases = [
  { scenarioId: 'baseline', changedSource: manifest.baselineSource, expectedTestExitCode: 0 },
  ...manifest.scenarios,
];

for (const scenario of cases) {
  test(`unchanged behavior tests expose the expected outcome: ${scenario.scenarioId}`, (context) => {
    const tempRoot = resolve(tmpdir());
    const projectRoot = mkdtempSync(join(tempRoot, 'tessera-checkout-fixture-'));
    context.after(() => {
      const childPath = relative(tempRoot, projectRoot);
      assert.ok(childPath.startsWith('tessera-checkout-fixture-') && !childPath.includes(sep));
      rmSync(projectRoot, { recursive: true, force: true });
    });
    for (const path of manifest.sandboxFiles) {
      const source = path === manifest.stagedPath ? scenario.changedSource : path;
      const destination = join(projectRoot, path);
      mkdirSync(dirname(destination), { recursive: true });
      copyFileSync(join(fixtureRoot, source), destination);
    }

    // The child is a separate test run, not a worker of this outer node:test run.
    const childEnvironment = { ...process.env };
    delete childEnvironment.NODE_TEST_CONTEXT;
    const result = spawnSync(process.execPath, ['--test', '--test-reporter=tap', manifest.behaviorTest], {
      cwd: projectRoot,
      env: childEnvironment,
      encoding: 'utf8',
      timeout: 10_000,
    });
    assert.equal(result.error, undefined);
    assert.equal(result.signal, null);
    assert.equal(result.status, scenario.expectedTestExitCode, result.stdout + result.stderr);
    assert.match(result.stdout, /# tests 3\b/);
    if (scenario.expectedTestExitCode === 1) {
      assert.match(result.stdout, /not ok 2 - rejects negative quantities/);
      assert.match(result.stdout, /Missing expected exception \(RangeError\)/);
      assert.match(result.stdout, /# pass 2\b/);
      assert.match(result.stdout, /# fail 1\b/);
    } else {
      assert.match(result.stdout, /# pass 3\b/);
      assert.match(result.stdout, /# fail 0\b/);
    }
  });
}
