# TEST.md — Verification Guide

**Last Updated:** 2026-04-26

Use this file when you want the shortest path to verify that Contextura still works after code or model changes.

## Fast Checks

Run the Rust test and compile gates first:

```bash
cargo test --manifest-path src-tauri/Cargo.toml
cargo check --manifest-path src-tauri/Cargo.toml
```

If you changed Rust runtime code, also run:

```bash
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings
```

## Translation Sidecar Probe

Start the bundled sidecar against the default TranslateGemma model:

```bash
./src-tauri/binaries/llama-server-aarch64-apple-darwin \
  --model ~/Library/Application\ Support/contextura/models/translategemma-4b-it.Q4_K_M.gguf \
  --port 8765 \
  --n-gpu-layers 99 \
  --ctx-size 1024 \
  --host 127.0.0.1 \
  --jinja
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
      {
        "role": "system",
        "content": "You are a Japanese-to-English translator. Respond with translation only."
      },
      {
        "role": "user",
        "content": [{ "type": "text", "text": "最近のAI技術の進歩により、リアルタイムでの多言語翻訳が可能になりました。" }],
        "source_lang_code": "ja",
        "target_lang_code": "en"
      }
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

After the app has captured at least one frame, verify the OCR/translation path on the saved PNG:

```bash
cargo run --manifest-path src-tauri/Cargo.toml -- \
  --debug-cli \
  --input /tmp/contextura-frame-latest.png \
  --pretty
```

The `--test-suite test-corpus` path is wired, but the current `test-corpus/*.png` assets are placeholders and should not be treated as a real regression suite yet.

## Manual Smoke Pass

Use a real screen containing Japanese text and confirm:

1. `cargo tauri dev` launches successfully.
2. Screen Recording permission is granted.
3. `/tmp/contextura-frame-latest.png` appears after a capture trigger.
4. `Cmd+Shift+R` forces an immediate scan on the cached frame.
5. Overlay text appears aligned over the original content.
6. `Cmd+Shift+M` clears translation memory and visible overlay state.
7. The overlay window does not show up inside the captured debug frame.

Do not mark the app as verified if only the Rust checks passed. End-to-end confirmation still requires a live GUI run with a valid local model.
