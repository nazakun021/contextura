# TODO.md — Contextura

**Stack:** Rust · Tauri v2 · ScreenCaptureKit · Swift `vision-helper` · `llama-server`  
**Platform:** macOS 13+ · Apple Silicon  
**Last Updated:** 2026-04-25

## Status Legend

- `[x]` Implemented and verified in code/tests in this workspace
- `[-]` Implemented, but still needs manual end-to-end runtime verification
- `[ ]` Not implemented

## Current Milestone

The single-display translation pipeline is wired, and the standalone OCR helper now works on saved live frames:

`capture -> motion debounce -> PNG snapshot -> OCR -> translation -> styling -> IPC overlay`

Most recent verification in this workspace:

- [x] `cargo test --manifest-path src-tauri/Cargo.toml`
- [x] `cargo check --manifest-path src-tauri/Cargo.toml`

Manual app-level smoke testing is still pending:

- [x] Launch app with a valid local model and confirm live translations appear over Japanese text - Live translations appear over japanese text, but it does not translate every japanese text in the screen, and it also does not translate the japanese the OCR detects. It translates some japanese the OCR detects, but it does not translate the whole.
- [x] Confirm app-switch invalidation clears overlay and translation memory - It clears the overlay, but I don't know if it also clears the translation memory.
- [ ] Confirm force OCR and overlay toggle hotkeys behave correctly in a live session - Does not work properly. It should start doing the OCR and translate when the hotkey is pressed. Instead, it does nothing.
- Engineering update 2026-04-25: `ForceScan` now runs immediately against the latest cached frame instead of waiting for the next capture tick. Live re-test still required.
- [x] Confirm the standalone `vision-helper` succeeds on `/tmp/contextura-frame-latest.png`

## Phase P.Complete

### Step 1 — PNG snapshot on debounce trigger

- [x] `image::{ImageBuffer, RgbaImage}` and `PathBuf` are used in `lib.rs`
- [x] `save_frame_as_png()` writes `/tmp/contextura-frame-{id}.png`
- [x] A persistent debug copy is kept at `/tmp/contextura-frame-latest.png`
- [x] First triggered frame now uses `frame_id = 0`
- [x] Capture requests `PixelFormat::BGRA` explicitly
- [x] BGRA → RGBA channel swap happens before PNG encode
- [x] Manual check: stop scrolling for ~300ms and confirm the saved image is a legible screenshot - Confirmed

### Step 2 — Motion detection wiring

- [x] `MotionDetector` is instantiated in `lib.rs`
- [x] `DebounceStateMachine` is instantiated in `lib.rs`
- [x] The frame loop gates OCR on debounce events
- [ ] Manual check: scrolling produces clear events and stopping produces a trigger after debounce - When I scroll, and I stop scrolling, it would not trigger the OCR and translation immediately, It's longer than the 300ms we set. I have to scroll up or down then go back to the position where i want to translate for it to tirgger.

### Step 3 — OCR and translation trigger branch

- [x] OCR uses the real saved PNG path
- [x] Temp PNG cleanup runs after OCR returns
- [x] Empty OCR results short-circuit the branch
- [x] Helper non-zero exit now surfaces as an OCR error instead of masquerading as empty OCR output
- [x] `translate_batch()` uses live OCR text
- [x] Manual check: the standalone helper returns OCR JSON on a real saved frame
- [ ] Manual check: a Japanese page produces OCR text and English translations - It works, but it does not translate all the japanese text the OCR detects. It translates some japanese text, but not all of them.

### Step 4 — Styling and IPC

- [x] `StylingEngine` is used when building overlay boxes
- [x] `translation-clear` is emitted on motion
- [x] `translation-started` is emitted before translation
- [x] `translation-update` is emitted with styled boxes
- [x] IPC structs in `ipc.rs` are live and no longer marked dead code
- [x] Manual check: overlay boxes render over detected Japanese text - It works, but it does not render over all the japanese text it detects. It only renders over some of the japanese text it detects.

### Step 5 — Context invalidation

- [x] App-switch invalidation drains before OCR
- [x] Manual reset clears translation memory without forcing overlay clear
- [x] Manual check: switching apps clears overlay; switching tabs inside the same app does not

### Step 6 — Functional hotkeys and tray actions

- [x] `Cmd+Shift+T` toggles overlay visibility
- [x] `Cmd+Shift+R` forces immediate OCR/translation
- [x] `Cmd+Shift+M` clears translation memory
- [x] Tray toggle works
- [x] Tray “Translate Now” works
- [x] Tray “Clear Context Memory” works
- [ ] Manual check: verify hotkeys and tray actions in a live app session - Overlay toggle works, but force ocr does not work. And I'm not sure if the translation memory was cleared when I press the hotkey.

### Step 7 — Stability cleanup

- [x] Panic hook cleans up `/tmp/contextura-frame-*.png`
- [x] Real display scale factor replaces the hardcoded `2.0`
- [x] Watchdog polls `/health` and restarts sidecar after repeated failures
- [x] `translation-error` now emits a structured payload
- [x] Battery detection uses `pmset -g batt`
- [x] Sentry is conditionally initialized via `CONTEXTURA_SENTRY_DSN`
- [ ] Manual check: simulate sidecar failure and confirm watchdog restart behavior

### Phase P.Complete Exit Criteria

- [-] Japanese text on screen translates after scrolling stops - Sometimes works, sometimes it does not.
- [-] Overlay clears on motion and app switch - Yes. When i change tabs in an app, it still persists for a time, but it will clear after some seconds. It clears when I switch apps.
- [-] Toggle and force-scan shortcuts work live - Toggle Works, force-scan in my testing does not work.

## Remaining Product Work

### High priority

- [ ] Exclude the overlay window from capture to avoid self-capture loops - I checked the latest png logs, you still capture the overlay window. It's like the overlay window is capturing itself in a recursive loop.
- Engineering update 2026-04-25: capture exclusion now prefers direct window exclusion and matches by bundle id, process id, and app-name hint. Live re-test still required.
- [x] Stabilize the standalone `vision-helper` runtime path on saved live frames
- [ ] Replace placeholder `test-corpus/*.png` files with real screenshots
- [ ] Run a full manual smoke pass with a real Qwen3 model and update verification checkboxes above
- [x] Make `translation-error` handling visible and user-friendly during sidecar restarts

### Medium priority

- [x] Implement `Cmd+Shift+G` model switching
- [x] Expand the first-run wizard beyond screen 1
- [x] Make `--debug-cli` run the real pipeline instead of stub output
- [x] Replace the mock `--test-suite` flow with real OCR/translation checks
- [ ] Curate `test-corpus/` with real Japanese screenshots and expected outputs
- [x] Handle sleep/wake and capture restarts more explicitly

### Lower priority

- [ ] Wire in-app updater configuration fully, including a real pubkey
- [ ] Revisit downloader integration
- [ ] Add multi-display support
- [-] Add quality-tier model switching
- [-] Cycle between installed models and surface tier labels is implemented; curated Standard/Quality policy and RAM gating are still pending
- [ ] Add Apple Foundation Models tier for macOS 26+

## File Status

- `src-tauri/src/lib.rs`: pipeline orchestrator, runtime reloads, real CLI/test-suite wiring, cached-frame force scan
- `src-tauri/src/models.rs`: model manifest loading and active-model switching
- `src-tauri/src/capture.rs`: ScreenCaptureKit capture, explicit BGRA, real scale factor, direct window/app exclusion
- `src-tauri/src/motion.rs`: motion detection and debounce, wired
- `src-tauri/src/ocr.rs`: `vision-helper` wrapper, wired, now surfaces helper failures explicitly
- `src-tauri/src/translation.rs`: sidecar management, batching, watchdog-ready
- `src-tauri/src/context.rs`: app-switch invalidation, wired
- `src-tauri/src/thermal.rs`: thermal plus battery detection
- `src-tauri/src/styling.rs`: WCAG-based overlay styling, wired
- `src-tauri/src/ipc.rs`: active IPC payload types
- `src-tauri/src/hotkeys.rs`: T/R/M/G/Q live
- `src-tauri/src/tray.rs`: primary tray actions plus model switch live
- `src/wizard.html`: 4-step setup flow live
- `src-tauri/src/bin/vision-helper.swift`: standalone OCR helper source, Cocoa-initialized and build-integrated
