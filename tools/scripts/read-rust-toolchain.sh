#!/usr/bin/env bash
# CI and release install the exact version selected by local rustup.
set -euo pipefail

channel=$(awk -F '"' '/^[[:space:]]*channel[[:space:]]*=/ { print $2 }' rust-toolchain.toml)
if ! [[ "$channel" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  printf 'rust-toolchain.toml must contain one exact Rust version\n' >&2
  exit 1
fi
printf '%s\n' "$channel"
