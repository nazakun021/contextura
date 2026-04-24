# ARCHITECTURE.md — Contextura

**Last Updated:** 2026-04-23

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
5. On trigger, `lib.rs` writes `/tmp/contextura-frame-{id}.png` and updates `/tmp/contextura-frame-latest.png`.
6. `ocr.rs` invokes the bundled Swift `vision-helper` and converts Vision coordinates to overlay coordinates.
7. `translation.rs` sends numbered translation batches to the local `llama-server` sidecar.
8. `styling.rs` samples background colors and computes readable foreground colors.
9. `ipc.rs` payloads are emitted to the overlay window.
10. `src/overlay.js` renders translated boxes into the transparent overlay DOM.

## Key Runtime Decisions

- Capture pixel format is explicitly `BGRA`.
- Snapshot encoding converts BGRA to RGBA before PNG save.
- Display scale factor is derived from ScreenCaptureKit display metadata, not hardcoded.
- Translation uses Qwen3-compatible `--jinja` plus `/no_think`.
- A watchdog restarts `llama-server` after repeated failed health checks.
- Context memory is cleared on app switch and manual reset.

## Modules

| Module | Role | Status |
| --- | --- | --- |
| `lib.rs` | Tauri setup and orchestration | Active |
| `capture.rs` | Screen frame capture | Active |
| `motion.rs` | Motion detection and debounce | Active |
| `ocr.rs` | OCR subprocess integration | Active |
| `translation.rs` | Sidecar lifecycle and translation batching | Active |
| `styling.rs` | Overlay contrast logic | Active |
| `context.rs` | App-switch invalidation | Active |
| `thermal.rs` | Thermal and battery throttling signals | Active |
| `hotkeys.rs` | Global shortcuts | Active |
| `tray.rs` | Tray controls | Active |
| `ipc.rs` | IPC payload contracts | Active |
| `downloader.rs` | Model download helper | Present but not integrated |
| `cli.rs` | CLI surface | Present but still limited |

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

- Swift binary
- Uses Apple Vision OCR
- Accepts a PNG path and returns JSON

### `llama-server`

- Local translation server
- Runs on `127.0.0.1:8765`
- Requires a decoder-only GGUF model
- Restarted by watchdog on repeated health-check failures

## Remaining Architectural Gaps

- Overlay window is not yet excluded from capture
- Model switching does not exist yet
- Wizard flow is still minimal
- Multi-display routing is not implemented
- CLI and test corpus flows are not yet full pipeline drivers
