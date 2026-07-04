# Contextura

Contextura is a macOS overlay that captures the screen, waits for motion to settle, runs OCR on Japanese text, translates it locally, and renders English boxes over the original content.

**Platform:** macOS 13+ on Apple Silicon  
**Stack:** Rust, Tauri v2, ScreenCaptureKit, Swift Vision helper, `llama-server`, vanilla HTML/CSS/JS  
**Status:** The single-display OCR-overlay pipeline is wired in code and remains the intended architecture. As of 2026-04-26, the repo also includes a TranslateGemma-specific translation path, AppKit-level overlay capture protection via `NSWindowSharingType::None`, a shared BGRA→RGBA conversion path for OCR and styling, and a debounce fix for inertial-scroll bleed. Live app verification is still required for end-to-end confirmation.

## Implemented In Code

- Screen capture through ScreenCaptureKit
- Motion-gated OCR/translation after debounce
- OCR subprocess integration through the bundled `vision-helper`
- Local translation through bundled `llama-server`, with model-specific handling for Qwen-style batched prompts and TranslateGemma structured requests
- Dynamic overlay styling for contrast
- Overlay toggle, cached-frame force-scan, memory reset, model switching, and quit hotkeys
- App-switch invalidation, watchdog-based sidecar restart, and capture-stream restart handling
- Overlay self-capture protection using direct window matching plus AppKit `NSWindowSharingType::None`
- A 4-step first-run wizard
- `--debug-cli --input` and `--test-suite` code paths routed through the live pipeline

## Known Active Issues

- The checked-in `test-corpus/*.png` fixtures are currently empty placeholder files and are not reliable verification assets.
- Manual runtime smoke verification is still pending with a valid local model.
- Force scan, context clearing, and overlay-exclusion behavior still need live confirmation in the running app after the latest translation/runtime fixes.

## Current Limits

- Single-display only
- Updater signing still needs a real public key
- Quality-tier policy and RAM gating are still incomplete
- End-to-end translation verification is still pending

## Setup

### 1. Install prerequisites

You need:

- Xcode
- Xcode Command Line Tools
- Rust
- Python 3

Quick check:

```bash
xcodebuild -version
xcode-select -p
rustc --version
cargo --version
python3 --version
```

### 2. Build the app

```bash
cargo tauri dev
```

The first build is slow because Tauri, ScreenCaptureKit bindings, and llama.cpp dependencies compile.

### 3. Download a compatible model

Contextura expects a decoder-only GGUF model. The current repo default uses **TranslateGemma 4B IT Q4_K_M**. Qwen-style GGUF models also work, but the docs and default settings are now centered on TranslateGemma.

```bash
python3 -m pip install huggingface_hub

huggingface-cli download mradermacher/translategemma-4b-it-GGUF \
  translategemma-4b-it.Q4_K_M.gguf \
  --local-dir ~/Library/Application\ Support/contextura/models/
```

Encoder-decoder models such as NLLB, MarianMT, T5, and BART do not work with the bundled `llama-server`.

### 4. Verify the sidecar manually

```bash
./src-tauri/binaries/llama-server-aarch64-apple-darwin \
  --model ~/Library/Application\ Support/contextura/models/translategemma-4b-it.Q4_K_M.gguf \
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

### 5. Grant Screen Recording permission

On first launch, Contextura shows a 4-step setup wizard covering Screen Recording permission, model placement, core shortcuts, and final readiness.

## Quick Verify

Run compile-time checks:

```bash
cargo test --manifest-path src-tauri/Cargo.toml
cargo check --manifest-path src-tauri/Cargo.toml
```

Then perform one live smoke pass:

1. Launch `cargo tauri dev`.
2. Open Japanese text on screen.
3. Stop moving for about `200ms`.
4. Confirm `/tmp/contextura-frame-latest.png` appears.
5. Confirm overlay translations render.
6. Confirm `Cmd+Shift+R` forces an immediate scan.

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
- OCR now fails explicitly on empty/corrupt PNGs and times out rather than hanging indefinitely.
- OCR post-processing keeps distinct overlapping text boxes and only removes near-duplicate detections.
- `llama-server` listens only on `127.0.0.1:8765`.
- TranslateGemma and Qwen3 both use `--jinja`, but only the Qwen path uses `/no_think`.
- TranslateGemma requests are sent sequentially within each chunk as structured chat messages instead of numbered text batches.
- Screen capture now also marks the overlay window as non-shareable through AppKit, instead of relying only on capture-filter exclusion.
- The debounce default is now `200ms`, and the settling phase requires larger motion before aborting.
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

See [docs/TEST.md](file:///Users/infinite/Developer/contextura/docs/TEST.md) for the focused verification workflow.

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

The standalone OCR helper currently rejects `/tmp/contextura-frame-latest.png` if it is empty or corrupt, which is now the intended behavior. Manual end-to-end app verification with a real model is still pending.

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
  downloader.rs
```

See the project constitution documents: [docs/MISSION.md](file:///Users/infinite/Developer/contextura/docs/MISSION.md), [docs/ROADMAP.md](file:///Users/infinite/Developer/contextura/docs/ROADMAP.md), and [docs/TECH-STACK.md](file:///Users/infinite/Developer/contextura/docs/TECH-STACK.md), along with [docs/SETUP.md](file:///Users/infinite/Developer/contextura/docs/SETUP.md) for setup instructions, [docs/TEST.md](file:///Users/infinite/Developer/contextura/docs/TEST.md) for testing, and [docs/SPEC.md](file:///Users/infinite/Developer/contextura/docs/SPEC.md) for technical contracts.
