#!/usr/bin/env bash
# Local baseline; CI also runs server, build, E2E, Docker and advisory suites.
# Rust checks are skipped locally only when Cargo is unavailable.

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
step "1/6  conflict-marker scan"
bash tools/scripts/pre-push-no-markers.sh || fail "unresolved Git conflict markers"
ok   "no conflict markers"

# 2. Dependency-free tooling and deterministic review fixtures.
step "2/6  tooling + review fixtures"
pnpm test:tooling || fail "release tooling tests failed"
pnpm test:eval-fixtures || fail "review fixture tests failed"
ok "tooling and fixture tests passed"

# 3. TypeScript across the monorepo.
step "3/6  pnpm typecheck"
pnpm typecheck || fail "TypeScript errors"
ok   "typecheck clean"

# 4. ESLint across the monorepo.
step "4/6  pnpm lint"
pnpm lint || fail "ESLint errors"
ok   "lint clean"

# 5. Frontend + shared unit tests only. Rust tests stay in step 6 so non-Rust
#    contributors are not blocked when cargo is unavailable locally.
step "5/6  frontend unit tests"
pnpm --filter @testing-ide/shared run test || fail "shared unit tests failed"
pnpm --filter @testing-ide/desktop run test:frontend || fail "frontend unit tests failed"
ok   "unit tests passed"

# 6. Rust clippy + unit tests, only if cargo is installed locally.
if command -v cargo >/dev/null 2>&1; then
  step "6/6  cargo clippy + cargo test --lib"
  (
    cd apps/desktop/src-tauri
    cargo clippy --locked --all-targets --lib -- -D warnings
    cargo test --locked --lib --quiet
  ) || fail "Rust checks failed"
  ok   "Rust checks passed"
else
  step "6/6  skip Rust checks"
  printf "\033[2m  cargo not found — skipping Rust checks (CI will still run them)\033[0m\n"
fi

printf "\n\033[1;32mAll local gates passed. Ready to push…\033[0m\n"
exit 0
