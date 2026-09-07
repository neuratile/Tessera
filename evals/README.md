# Review evaluation fixtures

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
