# TEST.md — Verification Guide

**Last Updated:** 2026-07-16

Use this file when you want the shortest path to verify that Contextura still works after code or model changes.

## Fast Checks

Run the Rust test and compile gates first:

```bash
cargo test --manifest-path src-tauri/Cargo.toml
cargo check --manifest-path src-tauri/Cargo.toml
```

Current workspace status at last verification: Rust test suite reports 107 passing tests.

## Local Pre-Push Lint Automation

Install repository-managed git hooks once:

```bash
./scripts/install-git-hooks.sh
```

After installation, every `git push` automatically runs:

1. `cargo fmt --all -- --check`
2. `cargo clippy --all-targets --all-features -- -D warnings -D clippy::perf`

If either check fails, the push is blocked locally.

## Automated Wire-To-Wire Smoke Test

Run the repository smoke harness that follows this verification guide end-to-end:

```bash
./scripts/smoke-wire-to-wire.sh
```

Quick mode (skips clippy, still runs build checks + single PNG probe + full corpus suite):

```bash
./scripts/smoke-wire-to-wire.sh --quick
```

What this verifies automatically:

1. Rust compile/test gates.
2. Single-image OCR + translation probe through `--debug-cli`.
3. Full `test-corpus` OCR + translation suite through `--debug-cli --test-suite`.

This is the recommended default smoke pass before any manual GUI validation.

If you changed Rust runtime code, also run clippy:

```bash
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings
```

## Translation Sidecar Probe

Start the bundled sidecar against the default TranslateGemma model (TranslateGemma strategy uses `--no-jinja`; Qwen/LFM strategies use `--jinja`):

```bash
./src-tauri/binaries/llama-server-aarch64-apple-darwin \
  --model ~/Library/Application\ Support/contextura/models/translategemma-4b-it.Q4_K_M.gguf \
  --port 8765 \
  --n-gpu-layers 99 \
  --ctx-size 1024 \
  --host 127.0.0.1 \
  --no-jinja
```

In another terminal, verify health:

```bash
curl http://127.0.0.1:8765/health
```

Run a direct translation request:

```bash
curl -X POST http://127.0.0.1:8765/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "local",
    "messages": [
      { "role": "system", "content": "You are a professional Japanese-to-English translator. Translate the user'\''s Japanese screen-text observations into natural, concise English. Output only the English translation of the observed text. Do not provide notes, explanations, or alternate translations." },
      { "role": "user", "content": "映画はとても面白かった。" }
    ],
    "temperature": 0.1,
    "max_tokens": 64
  }'
```

Stop the sidecar when done:

```bash
lsof -ti:8765 | xargs kill -9 2>/dev/null
```

## OCR And CLI Probe

Verify the OCR/translation path on a chosen PNG file (runtime OCR now streams in-memory PNG bytes to `vision-helper --stdin`):

```bash
cargo run --manifest-path src-tauri/Cargo.toml -- \
  --debug-cli \
  --input /absolute/path/to/sample.png \
  --pretty
```

To run the golden-file regression test suite against the live fixtures:

```bash
cargo run --manifest-path src-tauri/Cargo.toml -- \
  --debug-cli \
  --test-suite test-corpus
```

## Manual Smoke Pass

Use a real screen containing Japanese text and confirm:

1. `cargo tauri dev` launches successfully.
2. Screen Recording permission is granted.
3. A translation cycle runs successfully after a capture trigger (`Cmd+Shift+R` is the quickest probe).
4. `Cmd+Shift+R` forces an immediate scan on the cached frame.
5. Overlay text appears aligned over the original CJK content.
6. `Cmd+Shift+M` clears translation memory and visible overlay state.
7. The overlay window does not show up inside the captured debug frame.
8. **App Switching**: Verify switching apps clears overlay content and resets translation context as expected.
9. **Debounce Settle**: Verify the debounce behavior feels closer to the intended `200ms` settle time and no longer resets during active scrolling.
10. **Tray Controls**: Verify tray actions (toggle overlay, translate now, clear context) behave correctly.
11. **Watchdog Recovery**: Simulate a sidecar failure (e.g., run `pkill llama-server`) and confirm the watchdog restart notice is visible in the overlay and recovery completes successfully.

Do not mark the app as verified if only the Rust checks passed. End-to-end confirmation still requires a live GUI run with a valid local model.
