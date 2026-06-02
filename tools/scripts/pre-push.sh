#!/usr/bin/env bash
# Local pre-push gauntlet. Mirrors the required CI checks so that a
# green run here means the PR will go green on GitHub too.
#
# Stages, fastest first:
#   1. conflict-marker scan          (instant)
#   2. pnpm typecheck                (~10–30s, incremental via Turbo)
#   3. pnpm lint                     (~10–30s)
#   4. frontend unit tests           (Vitest only)
#   5. cargo clippy + cargo test     (only if cargo is installed)
#
# Anything fails → push is refused. Stage 5 is auto-skipped on machines
# without Rust so non-backend contributors are not blocked, but CI
# still runs it.

set -euo pipefail

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"

step() {
  printf "\n\033[1;36m▶ %s\033[0m\n" "$1"
}

ok() {
  printf "\033[1;32m✓ %s\033[0m\n" "$1"
}

fail() {
  printf "\n\033[1;31m✗ %s\033[0m\n" "$1" >&2
  printf "\033[2mFix the failure above and re-run \`git push\`.\033[0m\n" >&2
  printf "\033[2mEmergency bypass: \`git push --no-verify\` (CI will still gate the PR).\033[0m\n" >&2
  exit 1
}

# 1. Conflict markers — refuse to push merge artefacts.
step "1/5  conflict-marker scan"
bash tools/scripts/pre-push-no-markers.sh || fail "unresolved Git conflict markers"
ok   "no conflict markers"

# 2. TypeScript across the monorepo.
step "2/5  pnpm typecheck"
pnpm typecheck || fail "TypeScript errors"
ok   "typecheck clean"

# 3. ESLint across the monorepo.
step "3/5  pnpm lint"
pnpm lint || fail "ESLint errors"
ok   "lint clean"

# 4. Frontend unit tests only. Rust tests stay in step 5 so non-Rust
#    contributors are not blocked when cargo is unavailable locally.
step "4/5  frontend unit tests"
pnpm --filter @testing-ide/desktop run test:frontend || fail "frontend unit tests failed"
ok   "frontend unit tests passed"

# 5. Rust clippy + unit tests, only if cargo is installed locally.
if command -v cargo >/dev/null 2>&1; then
  step "5/5  cargo clippy + cargo test --lib"
  (
    cd apps/desktop/src-tauri
    cargo clippy --locked --all-targets --lib -- -D warnings
    cargo test --locked --lib --quiet
  ) || fail "Rust checks failed"
  ok   "Rust checks passed"
else
  step "5/5  cargo clippy"
  printf "\033[2m  cargo not found — skipping Rust checks (CI will still run them)\033[0m\n"
fi

printf "\n\033[1;32mAll local gates passed. Pushing…\033[0m\n"
exit 0
