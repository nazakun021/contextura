#!/bin/bash

# This script runs the same checks as the GitHub Actions CI workflow
# so you can catch failures locally before pushing.

set -e # Exit immediately if a command exits with a non-zero status.

echo "Running CI pre-push checks..."

cd src-tauri || { echo "❌ Could not find src-tauri directory!"; exit 1; }

echo "====================================="
echo "1. Checking code formatting..."
echo "====================================="
cargo fmt --all -- --check || {
    echo "❌ Formatting failed. Run 'cargo fmt --all' in src-tauri to fix."
    exit 1
}
echo "✅ Formatting is correct!"

echo ""
echo "====================================="
echo "2. Running Clippy lints..."
echo "====================================="
cargo clippy --all-targets --all-features -- -D warnings -D clippy::perf || {
    echo "❌ Clippy found issues. Please fix the warnings above."
    exit 1
}
echo "✅ Clippy checks passed!"

echo ""
echo "====================================="
echo "3. Running Unit Tests..."
echo "====================================="
cargo test || {
    echo "❌ Unit tests failed."
    exit 1
}
echo "✅ Unit tests passed!"

# echo ""
# echo "====================================="
# echo "4. Running E2E Test Suite..."
# echo "====================================="
# cargo run --bin contextura -- --debug-cli --test-suite ../test-corpus || {
#     echo "❌ E2E test suite failed."
#     exit 1
# }
# echo "✅ E2E tests passed!"

echo ""
echo "🎉 All checks passed! You are ready to push."
