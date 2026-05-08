#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "${script_dir}/../.." && pwd)"
desktop_root="${repo_root}/apps/desktop"
bundle_root="${desktop_root}/src-tauri/target/release/bundle"
output_root="${repo_root}/dist/desktop"

log() {
  printf '[deploy] %s\n' "$1"
}

require_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    printf '[deploy] missing required command: %s\n' "$1" >&2
    exit 1
  fi
}

signing_enabled() {
  [[ -n "${TAURI_SIGNING_PRIVATE_KEY:-}" ]] ||
    [[ -n "${APPLE_CERTIFICATE:-}" ]] ||
    [[ -n "${APPLE_SIGNING_IDENTITY:-}" ]] ||
    [[ -n "${WINDOWS_CERTIFICATE:-}" ]] ||
    [[ -n "${WINDOWS_CERTIFICATE_THUMBPRINT:-}" ]]
}

case "${output_root}" in
  "${repo_root}"/dist/desktop) ;;
  *)
    printf '[deploy] refusing to write outside repo dist/: %s\n' "${output_root}" >&2
    exit 1
    ;;
esac

require_cmd corepack
require_cmd npm

if [[ ! -d "${repo_root}/node_modules" ]]; then
  log "Installing workspace dependencies"
  (
    cd "${repo_root}"
    corepack pnpm install --frozen-lockfile
  )
fi

if signing_enabled; then
  log "Signing credentials detected; Tauri will sign supported artifacts."
else
  log "No signing credentials detected; building unsigned artifacts."
fi

log "Building desktop release bundle"
(
  cd "${desktop_root}"
  npm run build
)

if [[ ! -d "${bundle_root}" ]]; then
  printf '[deploy] expected bundle output at %s\n' "${bundle_root}" >&2
  exit 1
fi

rm -rf "${output_root}"
mkdir -p "${output_root}"
cp -R "${bundle_root}/." "${output_root}/"

log "Release artifacts copied to ${output_root}"
find "${output_root}" -maxdepth 3 -type f | sort
