#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

chmod +x .githooks/pre-push scripts/pre-push-lint.sh

git config core.hooksPath .githooks

printf "Git hooks installed. pre-push will now run lint checks automatically.\n"
