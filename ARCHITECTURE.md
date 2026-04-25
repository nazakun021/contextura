# ARCHITECTURE.md — Contextura

**Last Updated:** 2026-04-26

## Topology

```text
ScreenCaptureKit
  -> capture.rs
  -> motion.rs debounce gate
  -> lib.rs snapshot writer
  -> vision-helper OCR
  -> translation.rs sidecar client
  -> styling.rs
  -> ipc.rs payloads
  -> overlay.js in the Tauri overlay window
```

The backend runtime lives in `src-tauri/src/lib.rs`. `src-tauri/src/main.rs` is only a thin passthrough into `app_lib::run()`.

## End-to-End Flow

1. `capture.rs` starts an `SCStream` for the chosen display.
2. Frames are copied out of the pixel buffer as BGRA bytes.
3. `motion.rs` downsamples frames and computes motion ratio.
4. `DebounceStateMachine` decides whether to clear, wait, or trigger work.
5. On trigger, or on an explicit force-scan request, `lib.rs` swaps BGRA→RGBA once, writes `/tmp/contextura-frame-{id}.png`, and updates `/tmp/contextura-frame-latest.png`.
6. `ocr.rs` invokes the bundled Swift `vision-helper`, enforces helper timeouts/failure handling, and converts Vision coordinates to overlay coordinates without mutating Vision box geometry.
7. `translation.rs` selects a translation mode by active model: numbered batches for Qwen-style models, structured sequential requests for TranslateGemma.
8. `styling.rs` samples background colors from the same RGBA buffer used to write the OCR snapshot and computes readable foreground colors.
9. `ipc.rs` payloads are emitted to the overlay window.
10. `src/overlay.js` renders translated boxes into the transparent overlay DOM.

## Key Runtime Decisions

- Capture pixel format is explicitly `BGRA`.
- Snapshot encoding converts BGRA to RGBA once and reuses that RGBA buffer for styling.
- Display scale factor is derived from ScreenCaptureKit display metadata, not hardcoded.
- Translation uses model-specific prompting: Qwen-style numbered batches with `/no_think`, or TranslateGemma structured chat requests without `/no_think`.
- Force scan reuses the latest cached capture frame instead of waiting for another stream tick.
- A watchdog restarts `llama-server` after repeated failed health checks.
- Context memory is cleared on app switch and manual reset.
- Capture exclusion now prefers direct window exclusion for Contextura-owned windows, and the overlay window is also marked `NSWindowSharingType::None`.
- OCR post-processing sorts detections into stable reading order and only deduplicates near-identical boxes, preserving distinct overlapping text.
- Settling requires larger motion than the active scrolling threshold before debounce is cancelled, reducing inertial-scroll resets.

## Modules

| Module           | Role                                       | Status                     |
| ---------------- | ------------------------------------------ | -------------------------- |
| `lib.rs`         | Tauri setup and orchestration              | Active                     |
| `capture.rs`     | Screen frame capture                       | Active                     |
| `motion.rs`      | Motion detection and debounce              | Active                     |
| `ocr.rs`         | OCR subprocess integration and filtering   | Active                     |
| `translation.rs` | Sidecar lifecycle and translation batching | Active                     |
| `styling.rs`     | Overlay contrast logic                     | Active                     |
| `context.rs`     | App-switch invalidation                    | Active                     |
| `thermal.rs`     | Thermal and battery throttling signals     | Active                     |
| `hotkeys.rs`     | Global shortcuts                           | Active                     |
| `tray.rs`        | Tray controls                              | Active                     |
| `ipc.rs`         | IPC payload contracts                      | Active                     |
| `downloader.rs`  | Model download helper                      | Present but not integrated |
| `cli.rs`         | CLI surface                                | Active                     |

## Frontend

The frontend remains static and framework-free:

- `src/index.html`
- `src/overlay.js`
- `src/overlay.css`
- `src/wizard.html`
- `src/help.html`

The overlay listens for:

- `translation-started`
- `translation-update`
- `translation-clear`
- `translation-error`

## Sidecars

### `vision-helper`

- Swift binary in `src-tauri/src/bin/vision-helper.swift`
- Uses Apple Vision OCR
- Accepts an image path, validates that the file is readable/non-empty, and returns JSON OCR boxes on success
- Chooses among multiple Vision candidates per observation, favoring Japanese/CJK text when present

### `llama-server`

- Local translation server
- Runs on `127.0.0.1:8765`
- Requires a decoder-only GGUF model
- Default repo model target is `translategemma-4b-it.Q4_K_M.gguf`
- Restarted by watchdog on repeated health-check failures

## Remaining Architectural Gaps

- Live verification is still needed for the cached-frame force scan path
- Live verification is still needed to confirm overlay self-capture is fully gone in dev and packaged builds
- `test-corpus/` fixtures need to be replaced with real images
- Updater signing still needs a real public key
- Multi-display routing is not implemented
