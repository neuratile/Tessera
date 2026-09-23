# Review evaluation fixtures

## Existing artifact-generation quality (opt-in)

The separate `express-api` golden probe in `apps/desktop/tests/golden` uses Tessera's **existing test-case prompt and Ollama generation path**, not the planned staged-review engine. With pnpm dependencies, Rust/Cargo, a reachable local Ollama server and an installed supported chat model, run a single scored probe from the repository root:

```sh
TESSERA_GOLDEN_QUALITY_REPORT=1 pnpm --filter @testing-ide/desktop test:integration tests/golden/ollama-golden.integration.test.ts -t 'reports express auth'
```

PowerShell: set `$env:TESSERA_GOLDEN_QUALITY_REPORT='1'` before running the `pnpm ...` command. Set `OLLAMA_TEST_CHAT_MODEL` to select an installed model (otherwise the probe selects an installed supported qwen2.5-coder model); Ollama credentials are not required. The opt-in test is skipped without the flag; when requested but Ollama/Cargo is unavailable it fails instead of silently passing without a score. A successful run emits one `ARTIFACT_QUALITY_REPORT:` JSON line (save your terminal output if you need a persistent record). If generation fails, the test fails rather than inventing a score. Record the model, promptVersion and result for each run: model output is nondeterministic, even at the probe's temperature of 0.1. The scored test has no quality retries; the existing Rust probe can retry once on a transient stream interruption. This is **not** a CI gate or an aggregate provider benchmark.

`schemaValid` reports whether the emitted payload passes `TestCaseSchema`; `caseCount` and `textualScenarioCoverage` are reported separately. Five fixture-anchored scenarios are checked: successful login (token), missing login fields (400), bad credentials (400), logout of a known token (204), and logout of an unknown token (404). A scenario counts only when **one case** names a relevant input/action and puts the expected observation in a step's `expectedResult`. `matches[].caseIds` makes each hit auditable; misses are visible as empty arrays. Zero cases yields zero coverage. This lexical score is deliberately conservative and may miss semantically equivalent wording or count a plausible but unexecutable case. No generated `files[]` are executed, so the report makes **no assertion of runnable tests, source/line coverage, defect detection, or staged-review quality**. The scored auth behavior comes from `apps/desktop/tests/golden/fixtures/express-api/src`; health is outside the probe's `auth module` scope. The deterministic scorer regression check needs no model:

```sh
pnpm --filter @testing-ide/desktop test:frontend tests/golden/artifact-quality.test.ts
```

These synthetic fixtures define expected behavior for a future change-review
evaluator (#114). They do **not** show that Tessera's planned review engine detects
the seeded bugs. No evaluator, LLM calls, cloud credentials, Docker, or additional
dependencies are required here. Run the trusted fixture checks with Node.js 20+:

```sh
node --test evals/fixtures/checkout/fixture.test.mjs
# With workspace tooling installed:
pnpm test:eval-fixtures
```

The root `pnpm test` also runs these checks before the existing workspace suite.
They execute only the small, committed synthetic sources, not user projects.
The five outer checks pass; inside them the regression's unchanged behavior test
must fail specifically because a negative quantity does not throw `RangeError`.
This keeps CI green while detecting accidental removal of the seeded bug, broken
test imports, stale line references, and changes to the clean control's behavior.

## Checkout fixture

[`fixtures/checkout/manifest.json`](fixtures/checkout/manifest.json) records paths,
inputs, expected findings, and evidence expectations. Paths are relative to the
fixture directory, except `stagedPath`, `sandboxFiles`, and finding source paths,
which are relative to the materialized project. Source lines are one-based and
inclusive. This is fixture metadata, not a serialized review-session record.

- **Baseline:** `checkoutTotal(quantity)` rejects non-positive/non-integer counts
  and returns a total in cents at a fixed price of 1200 cents per item.
- **Regression:** the line 7 guard changes `quantity < 1` to `quantity === 0`.
  Input `-1` returns `-1200` cents instead of throwing `RangeError`.
- **Clean control:** reorders multiplication operands, preserving the guard and
  behavior. Expected findings are empty; a quantity-validation claim is a false
  positive for this change.
- **Behavior test:** `tests/checkout.test.mjs` remains identical across snapshots.
  Keep its negative-input assertion intact; rewriting it to accept `-1200` would
  erase the bug instead of proving a fix.

Static inspection can support a **suspected** finding. A future product finding
may be **reproduced** only after an execution demonstrates the assertion failure
against the exact reviewed source, with snapshot and test provenance. A compile,
import, environment, or unrelated test failure is **inconclusive** for this bug.
The local fixture check result is not product evidence. Runtime snapshot IDs,
provider/model identity, prompt version, and run IDs must come from an actual
review/run; the fixture does not invent them. Terminology is coordinated with
#105; this fixture has no runtime dependency on its new schemas.

## Materialize a staged repository for a later harness

Create a fresh temporary repository **per scenario**, commit only the baseline
source and unchanged behavior test, replace the source with the selected variant,
then stage that replacement. Review the index against `HEAD`; do not commit the
variant or mix in working-tree/untracked files. Keep `manifest.json`, the outer
fixture test, and the other snapshots outside the reviewed project so expected
answers cannot leak into model context.

Example PowerShell, starting at Tessera's repository root:

```powershell
$fixture = (Resolve-Path 'evals/fixtures/checkout').Path
$project = Join-Path ([IO.Path]::GetTempPath()) ('tessera-checkout-' + [guid]::NewGuid())
New-Item -ItemType Directory -Path (Join-Path $project 'src'), (Join-Path $project 'tests') | Out-Null
Copy-Item -LiteralPath (Join-Path $fixture 'baseline/src/checkout.mjs') -Destination (Join-Path $project 'src/checkout.mjs')
Copy-Item -LiteralPath (Join-Path $fixture 'tests/checkout.test.mjs') -Destination (Join-Path $project 'tests/checkout.test.mjs')
git -C $project init --quiet
git -C $project add -- src/checkout.mjs tests/checkout.test.mjs
git -C $project -c user.name='Fixture' -c user.email='fixture@example.invalid' -c commit.gpgsign=false commit -m 'fixture baseline'
# Use clean/src/checkout.mjs instead for the clean-control scenario.
Copy-Item -LiteralPath (Join-Path $fixture 'regression/src/checkout.mjs') -Destination (Join-Path $project 'src/checkout.mjs')
git -C $project add -- src/checkout.mjs
git -C $project diff --cached -- src/checkout.mjs
```

The later harness must read source bytes from the captured Git index snapshot,
record the baseline commit and staged blob identity, and derive source references
from that snapshot. Do not copy a subsequently edited working-tree file into a
run and label it the reviewed source.

For a sandbox run, package exactly `src/checkout.mjs` from the reviewed index and
the unchanged `tests/checkout.test.mjs` from the baseline. Run
`node --test tests/checkout.test.mjs` inside the project root. No dependency
installation or network access is needed. Preserve the test version and source
identity with output and exit status. Baseline/clean exit `0`; regression exits
`1` with only `rejects negative quantities` failing. An eventual sandbox adapter
must preserve this command and behavior or document an equivalent translation;
this issue does not add or claim that adapter exists.

Do not commit temporary repositories, `.git` directories, or generated files.
