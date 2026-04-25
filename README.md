# Contextura

Contextura is a macOS overlay that captures the screen, waits for motion to settle, runs OCR on Japanese text, translates it locally, and renders English boxes over the original content.

**Platform:** macOS 13+ on Apple Silicon  
**Stack:** Rust, Tauri v2, ScreenCaptureKit, Swift Vision helper, `llama-server`, vanilla HTML/CSS/JS  
**Status:** The single-display capture, translation, and overlay pipeline is wired in code. The bundled OCR helper now builds from source and was re-verified on `/tmp/contextura-frame-latest.png` in this workspace. On 2026-04-25, the force-scan path was updated to reuse the latest cached frame and capture exclusion was hardened to exclude matching app windows directly, but both still need live app verification.

## Implemented In Code

- Screen capture through ScreenCaptureKit
- Motion-gated OCR/translation after debounce
- OCR subprocess integration through the bundled `vision-helper`
- Local translation through bundled `llama-server`
- Dynamic overlay styling for contrast
- Overlay toggle, cached-frame force-scan, memory reset, model switching, and quit hotkeys
- App-switch invalidation, watchdog-based sidecar restart, and capture-stream restart handling
- Overlay self-capture exclusion using direct window matching plus app-level fallback
- A 4-step first-run wizard
- `--debug-cli --input` and `--test-suite` code paths routed through the live pipeline

## Known Active Issues

- The checked-in `test-corpus/*.png` fixtures are currently empty placeholder files and are not reliable verification assets.
- Manual runtime smoke verification is still pending with a valid local model.
- The 2026-04-25 force-scan and overlay-exclusion fixes need live confirmation in the running app.

## Current Limits

- Single-display only
- Updater signing still needs a real public key
- Quality-tier policy and RAM gating are still incomplete
- End-to-end translation verification is still pending

## Setup

### 1. Build the app

```bash
cargo tauri dev
```

The first build is slow because Tauri, ScreenCaptureKit bindings, and llama.cpp dependencies compile.

### 2. Download a compatible model

Contextura expects a decoder-only GGUF model. The default setup uses **Qwen3-0.6B Q4_K_M**.

```bash
pip install huggingface_hub

huggingface-cli download Qwen/Qwen3-0.6B-GGUF \
  qwen3-0.6b-q4_k_m.gguf \
  --local-dir ~/Library/Application\ Support/contextura/models/
```

Encoder-decoder models such as NLLB, MarianMT, T5, and BART do not work with the bundled `llama-server`.

### 3. Verify the sidecar manually

```bash
./src-tauri/binaries/llama-server-aarch64-apple-darwin \
  --model ~/Library/Application\ Support/contextura/models/qwen3-0.6b-q4_k_m.gguf \
  --port 8765 \
  --n-gpu-layers 99 \
  --ctx-size 1024 \
  --host 127.0.0.1 \
  --jinja
```

In another terminal:

```bash
curl http://127.0.0.1:8765/health
```

Expected:

```json
{ "status": "ok" }
```

### 4. Grant Screen Recording permission

On first launch, Contextura shows a 4-step setup wizard covering Screen Recording permission, model placement, core shortcuts, and final readiness.

## Hotkeys

| Shortcut      | Action                                   | Status          |
| ------------- | ---------------------------------------- | --------------- |
| `Cmd+Shift+T` | Toggle overlay visibility                | Live            |
| `Cmd+Shift+R` | Force immediate OCR/translation          | Re-test pending |
| `Cmd+Shift+M` | Clear translation memory                 | Live            |
| `Cmd+Shift+Q` | Quit                                     | Live            |
| `Cmd+Shift+G` | Switch to the next installed local model | Live            |

## Runtime Notes

- The app writes numbered snapshots to `/tmp/contextura-frame-{id}.png` during OCR passes.
- The latest captured frame is also kept at `/tmp/contextura-frame-latest.png` for debugging.
- `llama-server` listens only on `127.0.0.1:8765`.
- Qwen3 uses `--jinja`, and translation requests include `/no_think` in the system prompt.
- Screen capture now prefers excluding matching Contextura windows directly, with app-level exclusion as fallback, to avoid self-capture loops.
- If capture stalls after display sleep/wake or a permission reset, the runtime rebuilds the capture stream.

## CLI

Run one real OCR/translation pass against a PNG:

```bash
cargo run --manifest-path src-tauri/Cargo.toml -- \
  --debug-cli \
  --input /tmp/contextura-frame-latest.png \
  --pretty
```

Run the bundled corpus checks with the active local model:

```bash
cargo run --manifest-path src-tauri/Cargo.toml -- \
  --debug-cli \
  --test-suite test-corpus
```

The current `test-corpus/` PNG fixtures are placeholders, so this command path is wired but not yet a trustworthy regression suite.

## Optional Crash Reporting

Sentry is disabled by default. To enable it for a session:

```bash
export CONTEXTURA_SENTRY_DSN="<your sentry dsn>"
cargo tauri dev
```

## Verification

Most recent verification in this workspace:

```bash
cargo test --manifest-path src-tauri/Cargo.toml
cargo check --manifest-path src-tauri/Cargo.toml
```

The standalone OCR helper was also re-run successfully against `/tmp/contextura-frame-latest.png`. Manual end-to-end app verification with a real model is still pending.

## Project Layout

```text
src/
  index.html
  overlay.js
  overlay.css
  wizard.html
  help.html

src-tauri/src/
  lib.rs
  models.rs
  capture.rs
  motion.rs
  ocr.rs
  translation.rs
  styling.rs
  context.rs
  thermal.rs
  hotkeys.rs
  tray.rs
  settings.rs
  ipc.rs
  cli.rs
```

See `SPEC.md` for current contracts and `ARCHITECTURE.md` for data flow.
