#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
MANIFEST_PATH="$ROOT_DIR/src-tauri/Cargo.toml"
CORPUS_DIR="$ROOT_DIR/test-corpus"
PROBE_IMAGE="$CORPUS_DIR/case1-dialog.png"
PROBE_JSON="$ROOT_DIR/.smoke-probe-output.json"
RUN_CLIPPY=1

print_step() {
  printf "\n==> %s\n" "$1"
}

fail() {
  printf "\n[SMOKE FAIL] %s\n" "$1" >&2
  exit 1
}

for arg in "$@"; do
  case "$arg" in
    --quick)
      RUN_CLIPPY=0
      ;;
    *)
      fail "Unknown argument: $arg"
      ;;
  esac
done

print_step "Preflight checks"
[[ -f "$MANIFEST_PATH" ]] || fail "Missing manifest at $MANIFEST_PATH"
[[ -d "$CORPUS_DIR" ]] || fail "Missing corpus dir at $CORPUS_DIR"
[[ -f "$PROBE_IMAGE" ]] || fail "Missing probe image at $PROBE_IMAGE"
command -v cargo >/dev/null 2>&1 || fail "cargo is required"
command -v python3 >/dev/null 2>&1 || fail "python3 is required"

print_step "Rust fast gates from docs"
cargo check --manifest-path "$MANIFEST_PATH"
cargo test --manifest-path "$MANIFEST_PATH" -q

if [[ "$RUN_CLIPPY" -eq 1 ]]; then
  print_step "Rust strict lint gate"
  cargo clippy --manifest-path "$MANIFEST_PATH" --all-targets --all-features -- -D warnings
else
  print_step "Skipping clippy (--quick)"
fi

print_step "Wire-to-wire probe: single PNG OCR + translation"
cargo run --manifest-path "$MANIFEST_PATH" -- \
  --debug-cli \
  --input "$PROBE_IMAGE" \
  --pretty > "$PROBE_JSON"

python3 - <<'PY' "$PROBE_JSON"
import json
import sys
path = sys.argv[1]
with open(path, "r", encoding="utf-8") as f:
    data = json.load(f)
ocr = data.get("ocr") or []
translations = data.get("translations") or []
if not ocr:
    raise SystemExit("Probe output has no OCR entries")
if not translations:
    raise SystemExit("Probe output has no translation entries")
if len(ocr) != len(translations):
    raise SystemExit("Probe output OCR/translation length mismatch")
if any((not isinstance(t, str)) or (not t.strip()) for t in translations):
    raise SystemExit("Probe output contains empty translation strings")
print(f"Probe OK: {len(ocr)} OCR boxes, {len(translations)} translations")
PY

print_step "Wire-to-wire verification: full corpus suite"
cargo run --manifest-path "$MANIFEST_PATH" -- \
  --debug-cli \
  --test-suite "$CORPUS_DIR"

print_step "Smoke test complete"
printf "[SMOKE PASS] Wire-to-wire verification succeeded.\n"
