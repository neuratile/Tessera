# Tessera roadmap

Priority: **Review my changes**, a staged Git review with code-linked findings
and evidence tied to exact source/test versions. See
[epic #104](https://github.com/neuratile/Tessera/issues/104) and the
[contract](./versions/v2/AI_FIRST_REVIEW.md).

Existing generation, provider selection, exports, sandbox, flaky checks, healing,
and mutation features remain available. [Project status](../docs/PROJECT_STATUS.md)
records capabilities; a plan folder does not imply a published release.

## Foundations available

- [#105](https://github.com/neuratile/Tessera/issues/105): staged-review contract.
- [#113](https://github.com/neuratile/Tessera/issues/113):
  [checkout fixture](../evals/README.md), seeded regression and clean control.
- [#95](https://github.com/neuratile/Tessera/issues/95): expanded sandbox boundary tests.

The new review runtime is not implemented yet.

## Delivery order

| Stage | Work | Issues |
|---|---|---|
| 1 | Read-only Git capture; contracts/persistence | [#106](https://github.com/neuratile/Tessera/issues/106), [#107](https://github.com/neuratile/Tessera/issues/107) |
| 2 | Bounded context; code-linked findings | [#108](https://github.com/neuratile/Tessera/issues/108), [#109](https://github.com/neuratile/Tessera/issues/109) |
| 3 | Workflow/IPC; desktop UI | [#110](https://github.com/neuratile/Tessera/issues/110), [#111](https://github.com/neuratile/Tessera/issues/111) |
| 4 | Execution evidence bound to captured source/tests | [#112](https://github.com/neuratile/Tessera/issues/112) |
| 5 | Evaluation harness; first-review walkthrough | [#114](https://github.com/neuratile/Tessera/issues/114), [#115](https://github.com/neuratile/Tessera/issues/115) |
| Later | Reusable core, CLI, MCP | [#116](https://github.com/neuratile/Tessera/issues/116), [#117](https://github.com/neuratile/Tessera/issues/117), [#118](https://github.com/neuratile/Tessera/issues/118) |

The contract defines dependencies and acceptance details. Contributor setup can
improve now; a real review walkthrough depends on runtime delivery.

## Acceptance principles

- Capture index versus HEAD without modifying Git or working files.
- Preserve the selected provider, original line locations, and context/cost limits.
- Separate suspected findings, reproduced evidence, and inconclusive execution.
- Never equate regenerated passing tests with application repair.
- Measure bug recall, clean-control false positives, test validity, time, and cost.

Automatic regeneration, multi-model consensus, broad collaboration, more trackers
and runners, and vector-index migration are deferred. Accessibility, E2E, and
artifact freshness remain relevant supporting work. Earlier
[vision notes](./versions/v2/V2_VISION.md) are history; this roadmap sets priority.
