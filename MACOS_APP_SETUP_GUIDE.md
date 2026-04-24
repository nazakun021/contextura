# Contextura macOS Setup Guide

This guide is the shortest path from "I cloned the repo" to "the app captures my screen and shows translations."

It is written for your current stack:

- macOS app shell: Tauri v2
- backend/runtime: Rust
- screen capture: ScreenCaptureKit
- OCR: bundled Swift `vision-helper`
- translation: bundled `llama-server`
- frontend overlay: vanilla HTML/CSS/JS

Use this document as your working checklist.

## 1. What Has To Work

For Contextura to work end-to-end, all of these must be true:

1. The app builds successfully.
2. macOS Screen Recording permission is granted.
3. A compatible local GGUF model exists on disk.
4. `llama-server` starts and responds on `127.0.0.1:8765`.
5. The app captures a frame.
6. OCR finds Japanese text.
7. Translation runs.
8. The overlay receives IPC events and renders boxes.

If one of those fails, the app will look "broken" even if the rest is correct.

## 2. What You Need Installed

Before touching app code, install these:

- Xcode
- Xcode Command Line Tools
- Rust toolchain
- Python 3

Check them:

```bash
xcodebuild -version
xcode-select -p
rustc --version
cargo --version
python3 --version
```

If Xcode Command Line Tools are missing:

```bash
xcode-select --install
```

## 3. Understand The Architecture First

Your app flow today is:

```text
ScreenCaptureKit
  -> capture.rs
  -> motion.rs
  -> save PNG to /tmp
  -> vision-helper
  -> translation.rs
  -> IPC events
  -> src/overlay.js
```

Important files:

- [src-tauri/src/lib.rs](/Users/infinite/Programming/contextura/src-tauri/src/lib.rs:88): main runtime wiring
- [src-tauri/src/capture.rs](/Users/infinite/Programming/contextura/src-tauri/src/capture.rs:1): screen capture
- [src-tauri/src/ocr.rs](/Users/infinite/Programming/contextura/src-tauri/src/ocr.rs:1): OCR helper integration
- [src-tauri/src/translation.rs](/Users/infinite/Programming/contextura/src-tauri/src/translation.rs:59): sidecar startup and translation requests
- [src/overlay.js](/Users/infinite/Programming/contextura/src/overlay.js:1): frontend rendering of results

If you are new to macOS development, think of this app as three parts:

1. Native OS access
2. Local AI pipeline
3. Overlay UI

Do not debug all three at once.

## 4. First Build

From the repo root:

```bash
cargo tauri dev
```

The first build can be slow.

What a successful first run means:

- the Rust app compiled
- the Tauri window process launched
- the bundled binaries were found

What it does not guarantee:

- screen capture permission works
- OCR works
- translation works

## 5. Grant macOS Permission

This app needs Screen Recording permission.

On first run, macOS should ask. If it does not, check manually:

```text
System Settings -> Privacy & Security -> Screen Recording
```

Enable permission for your dev app or terminal as needed.

Without this, `ScreenCaptureKit` can fail or return unusable capture results.

## 6. Install A Compatible Model

This app uses `llama-server`, so the model must be a decoder-only GGUF model.

Use the current default:

- `Qwen/Qwen3-0.6B-GGUF`
- file: `qwen3-0.6b-q4_k_m.gguf`

Install the Python helper if needed:

```bash
python3 -m pip install huggingface_hub
```

Download the model:

```bash
huggingface-cli download Qwen/Qwen3-0.6B-GGUF \
  qwen3-0.6b-q4_k_m.gguf \
  --local-dir ~/Library/Application\ Support/contextura/models/
```

After download, you should have a file roughly here:

```text
~/Library/Application Support/contextura/models/qwen3-0.6b-q4_k_m.gguf
```

## 7. Verify The Translation Sidecar Alone

Do this before testing the full app.

Run:

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

If this fails, do not debug the overlay yet. Fix the sidecar first.

## 8. Run The Full App

Once the model exists and `/health` works:

```bash
cargo tauri dev
```

Open a screen with Japanese text.

Then verify this behavior:

1. Scroll or move the content.
2. Stop moving.
3. Wait about 300ms.
4. The app should save a screenshot, run OCR, translate text, and show overlay boxes.

## 9. Check The Fastest Debug Signals

When the app seems broken, check these in order.

### A. Was a frame captured?

Look for:

```text
/tmp/contextura-frame-latest.png
```

If that file does not appear after a trigger, the issue is before OCR.

Likely area:

- capture
- debounce logic
- trigger path in `lib.rs`

### B. Does the screenshot look correct?

If the image is black, empty, or wrong:

- Screen Recording permission may be missing
- capture may be targeting the wrong display later on
- scaling or frame extraction may be wrong

### C. Does OCR run?

If a PNG exists but nothing translates, the next suspect is `vision-helper`.

Likely area:

- [src-tauri/src/ocr.rs](/Users/infinite/Programming/contextura/src-tauri/src/ocr.rs:58)

### D. Does translation run?

If OCR is working but translations do not appear:

- verify `llama-server` is up
- verify the model path exists
- verify `translation.rs` parses the model response format correctly

Likely area:

- [src-tauri/src/translation.rs](/Users/infinite/Programming/contextura/src-tauri/src/translation.rs:137)

### E. Does the overlay receive events?

The frontend listens for:

- `translation-started`
- `translation-update`
- `translation-clear`
- `translation-error`

See:

- [src/overlay.js](/Users/infinite/Programming/contextura/src/overlay.js:8)

If backend logs look fine but nothing appears onscreen, the bug is probably in event delivery or DOM rendering.

## 10. Hotkeys You Can Use Right Now

Implemented:

- `Cmd+Shift+T`: toggle overlay
- `Cmd+Shift+R`: force immediate OCR/translation
- `Cmd+Shift+M`: clear translation memory
- `Cmd+Shift+Q`: quit app

Not implemented yet:

- `Cmd+Shift+G`: model switching stub only

Source:

- [src-tauri/src/hotkeys.rs](/Users/infinite/Programming/contextura/src-tauri/src/hotkeys.rs:1)

For debugging, `Cmd+Shift+R` is your most useful shortcut.

## 11. What "Working" Means At Each Stage

### Stage 1: Build works

You can run:

```bash
cargo tauri dev
```

### Stage 2: Capture works

You can see:

```text
/tmp/contextura-frame-latest.png
```

### Stage 3: OCR works

Japanese text on the saved frame produces OCR boxes internally.

### Stage 4: Translation works

`llama-server` is healthy and returns English text.

### Stage 5: Overlay works

You see translated boxes on screen.

Do not skip stages. Verify each one separately.

## 12. What To Learn First As A New macOS Developer

Do not try to learn every Apple framework at once. For this project, learn in this order:

1. Tauri app lifecycle
2. Rust error handling and async basics
3. macOS permissions model
4. ScreenCaptureKit basics
5. How subprocesses are launched and managed
6. Basic WebView overlay behavior

For this repo specifically:

- learn how `lib.rs` wires modules together
- learn how `capture.rs` hands frames into the pipeline
- learn how `translation.rs` manages the sidecar
- learn how `overlay.js` renders results

## 13. Personal Use First, Production Later

This is the right strategy.

For personal use, your goal is:

- stable local run
- good enough translations
- acceptable latency
- easy restart when something breaks

For production, your goal becomes:

- graceful errors instead of crashes
- real onboarding flow
- model management UX
- updater signing
- stronger test coverage
- sandbox and packaging correctness

Do not optimize for App Store quality yet.

## 14. Current High-Value Next Steps

Follow these in order.

### Immediate

1. Run the app with a real Qwen3 model.
2. Confirm `/tmp/contextura-frame-latest.png` is generated.
3. Confirm a real Japanese screen produces overlay translations.
4. Confirm `Cmd+Shift+R` works during a live session.

### After That

1. Fix overlay exclusion from capture.
2. Improve startup error handling instead of relying on `expect` and `unwrap`.
3. Replace the mocked CLI E2E flow with real checks.
4. Add a better wizard for permissions and model setup.

## 15. Common Mistakes To Avoid

- Using the wrong model architecture
- Debugging UI before verifying the sidecar
- Assuming build success means runtime success
- Changing multiple subsystems at once
- Adding more features before completing one manual end-to-end smoke test

## 16. Your Practical Workflow

Use this loop:

1. Make one small change.
2. Run `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings`.
3. Run `cargo test --manifest-path src-tauri/Cargo.toml`.
4. Launch `cargo tauri dev`.
5. Test on a real Japanese screen.
6. Check `/tmp/contextura-frame-latest.png`.
7. Read logs.
8. Repeat.

That loop is enough to get this app from "compiles" to "actually useful."

## 17. Useful Repo Docs

- [README.md](/Users/infinite/Programming/contextura/README.md:1)
- [SPEC.md](/Users/infinite/Programming/contextura/SPEC.md:1)
- [ARCHITECTURE.md](/Users/infinite/Programming/contextura/ARCHITECTURE.md:1)
- [TODO.md](/Users/infinite/Programming/contextura/TODO.md:1)
- [PRODUCTION.md](/Users/infinite/Programming/contextura/PRODUCTION.md:1)

## 18. If You Feel Stuck

When the app fails, answer these five questions first:

1. Did the app build?
2. Did macOS allow screen recording?
3. Did a PNG appear in `/tmp`?
4. Did `llama-server` return `{"status":"ok"}`?
5. Did `translation-update` reach the overlay?

That is usually enough to narrow the bug to one subsystem instead of guessing.
