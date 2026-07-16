#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"

printf "==> Running pre-push lint checks\n"
cd "$ROOT_DIR/src-tauri"

printf "==> cargo fmt --all -- --check\n"
cargo fmt --all -- --check

printf "==> cargo clippy --all-targets --all-features -- -D warnings -D clippy::perf\n"
cargo clippy --all-targets --all-features -- -D warnings -D clippy::perf

printf "[PRE-PUSH PASS] Lint checks succeeded.\n"
