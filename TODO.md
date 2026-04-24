# TODO.md — Contextura

**Stack:** Rust · Tauri v2 · ScreenCaptureKit · Swift `vision-helper` · `llama-server`  
**Platform:** macOS 13+ · Apple Silicon  
**Last Updated:** 2026-04-23

## Status Legend

- `[x]` Implemented and verified in code/tests in this workspace
- `[-]` Implemented, but still needs manual end-to-end runtime verification
- `[ ]` Not implemented

## Current Milestone

The single-display translation pipeline is now wired:

`capture -> motion debounce -> PNG snapshot -> OCR -> translation -> styling -> IPC overlay`

Rust verification completed in this workspace:

- [x] `cargo test --manifest-path src-tauri/Cargo.toml`
- [x] `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings`

Manual app-level smoke testing is still pending:

- [ ] Launch app with a valid local model and confirm live translations appear over Japanese text
- [ ] Confirm app-switch invalidation clears overlay and translation memory
- [ ] Confirm force OCR and overlay toggle hotkeys behave correctly in a live session

## Phase P.Complete

### Step 1 — PNG snapshot on debounce trigger

- [x] `image::{ImageBuffer, RgbaImage}` and `PathBuf` are used in `lib.rs`
- [x] `save_frame_as_png()` writes `/tmp/contextura-frame-{id}.png`
- [x] A persistent debug copy is kept at `/tmp/contextura-frame-latest.png`
- [x] First triggered frame now uses `frame_id = 0`
- [x] Capture requests `PixelFormat::BGRA` explicitly
- [x] BGRA → RGBA channel swap happens before PNG encode
- [ ] Manual check: stop scrolling for ~300ms and confirm the saved image is a legible screenshot

### Step 2 — Motion detection wiring

- [x] `MotionDetector` is instantiated in `lib.rs`
- [x] `DebounceStateMachine` is instantiated in `lib.rs`
- [x] The frame loop gates OCR on debounce events
- [ ] Manual check: scrolling produces clear events and stopping produces a trigger after debounce

### Step 3 — OCR and translation trigger branch

- [x] OCR uses the real saved PNG path
- [x] Temp PNG cleanup runs after OCR returns
- [x] Empty OCR results short-circuit the branch
- [x] `translate_batch()` uses live OCR text
- [ ] Manual check: a Japanese page produces OCR text and English translations

### Step 4 — Styling and IPC

- [x] `StylingEngine` is used when building overlay boxes
- [x] `translation-clear` is emitted on motion
- [x] `translation-started` is emitted before translation
- [x] `translation-update` is emitted with styled boxes
- [x] IPC structs in `ipc.rs` are live and no longer marked dead code
- [ ] Manual check: overlay boxes render over detected Japanese text

### Step 5 — Context invalidation

- [x] App-switch invalidation drains before OCR
- [x] Manual reset clears translation memory without forcing overlay clear
- [ ] Manual check: switching apps clears overlay; switching tabs inside the same app does not

### Step 6 — Functional hotkeys and tray actions

- [x] `Cmd+Shift+T` toggles overlay visibility
- [x] `Cmd+Shift+R` forces immediate OCR/translation
- [x] `Cmd+Shift+M` clears translation memory
- [x] Tray toggle works
- [x] Tray “Translate Now” works
- [x] Tray “Clear Context Memory” works
- [ ] Manual check: verify hotkeys and tray actions in a live app session

### Step 7 — Stability cleanup

- [x] Panic hook cleans up `/tmp/contextura-frame-*.png`
- [x] Real display scale factor replaces the hardcoded `2.0`
- [x] Watchdog polls `/health` and restarts sidecar after repeated failures
- [x] `translation-error` now emits a structured payload
- [x] Battery detection uses `pmset -g batt`
- [x] Sentry is conditionally initialized via `CONTEXTURA_SENTRY_DSN`
- [ ] Manual check: simulate sidecar failure and confirm watchdog restart behavior

### Phase P.Complete Exit Criteria

- [-] Japanese text on screen translates after scrolling stops
- [-] Overlay clears on motion and app switch
- [-] Toggle and force-scan shortcuts work live

## Remaining Product Work

### High priority

- [ ] Exclude the overlay window from capture to avoid self-capture loops
- [ ] Run a full manual smoke pass with a real Qwen3 model and update verification checkboxes above
- [ ] Make `translation-error` handling visible and user-friendly during sidecar restarts

### Medium priority

- [ ] Implement `Cmd+Shift+G` model switching
- [ ] Expand the first-run wizard beyond screen 1
- [ ] Make `--debug-cli` run the real pipeline instead of stub output
- [ ] Replace the mock `--test-suite` flow with real OCR/translation checks
- [ ] Curate `test-corpus/` with real Japanese screenshots and expected outputs
- [ ] Handle sleep/wake and capture restarts more explicitly

### Lower priority

- [ ] Wire in-app updater configuration fully, including a real pubkey
- [ ] Revisit downloader integration
- [ ] Add multi-display support
- [ ] Add quality-tier model switching
- [ ] Add Apple Foundation Models tier for macOS 26+

## File Status

- `src-tauri/src/lib.rs`: pipeline orchestrator, wired
- `src-tauri/src/capture.rs`: ScreenCaptureKit capture, explicit BGRA, real scale factor
- `src-tauri/src/motion.rs`: motion detection and debounce, wired
- `src-tauri/src/ocr.rs`: `vision-helper` wrapper, wired
- `src-tauri/src/translation.rs`: sidecar management, batching, watchdog-ready
- `src-tauri/src/context.rs`: app-switch invalidation, wired
- `src-tauri/src/thermal.rs`: thermal plus battery detection
- `src-tauri/src/styling.rs`: WCAG-based overlay styling, wired
- `src-tauri/src/ipc.rs`: active IPC payload types
- `src-tauri/src/hotkeys.rs`: T/R/M/Q live, G still stub
- `src-tauri/src/tray.rs`: primary tray actions live
- `src/wizard.html`: initial permission flow only
