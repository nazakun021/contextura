# SPEC.md — Contextura

**Version:** 1.8.0  
**Last Updated:** 2026-04-25  
**Target:** macOS 13+ on Apple Silicon

## Summary

Contextura is a local-only screen translation overlay for Japanese text on macOS. The current implementation targets a single display and uses:

- ScreenCaptureKit for screen frames
- A Rust motion/debounce pipeline to avoid OCR during scrolling
- A Swift `vision-helper` subprocess for OCR
- A local `llama-server` sidecar for translation
- A Tauri overlay window for rendering translated boxes

## Current Implementation Status

### Verified in this workspace

- `cargo test --manifest-path src-tauri/Cargo.toml` passes
- `cargo check --manifest-path src-tauri/Cargo.toml` passes
- Standalone `vision-helper` returns OCR JSON on `/tmp/contextura-frame-latest.png`

### Code-integrated features

| Area                          | Status | Notes                                                                                                     |
| ----------------------------- | ------ | --------------------------------------------------------------------------------------------------------- |
| Tauri app bootstrap           | ✅     | `main.rs` stays thin; `lib.rs` owns runtime                                                               |
| Screen capture                | ✅     | Single display, explicit BGRA                                                                             |
| Motion detection and debounce | ✅     | Wired into frame loop                                                                                     |
| PNG snapshot writing          | ✅     | Temp file plus persistent latest debug copy                                                               |
| OCR subprocess                | ✅     | Bundled `vision-helper` builds from source and returns OCR JSON on a saved live frame                     |
| Translation sidecar           | ✅     | Qwen3-compatible args, health check, restart support                                                      |
| Dynamic styling               | ✅     | WCAG-based foreground/background selection                                                                |
| IPC to overlay                | ✅     | `translation-started`, `translation-update`, `translation-clear`, `translation-error`                     |
| Overlay toggle hotkey         | ✅     | `Cmd+Shift+T`                                                                                             |
| Force OCR hotkey              | ✅     | `Cmd+Shift+R`; 2026-04-25 patch now reuses the latest cached frame, live re-check pending                 |
| Manual memory reset           | ✅     | `Cmd+Shift+M`                                                                                             |
| Tray primary actions          | ✅     | Toggle, translate now, clear context                                                                      |
| Model switching               | ✅     | `Cmd+Shift+G` cycles to next installed local model                                                        |
| Context invalidation          | ✅     | App switch clears memory and overlay                                                                      |
| Watchdog                      | ✅     | Restarts sidecar after repeated health failures                                                           |
| Overlay capture exclusion     | ✅     | Capture now excludes matching Contextura windows directly, with app-level fallback; live re-check pending |
| Wizard screens 1–4            | ✅     | Setup flow covers permissions, model, controls, ready state                                               |
| Real CLI OCR/translation path | ⚠️     | Code path is live, but end-to-end verification still depends on sidecar readiness and a valid corpus      |
| Capture restart handling      | ✅     | Stalled capture stream triggers restart path                                                              |
| Thermal + battery awareness   | ✅     | Thermal API + `pmset -g batt`                                                                             |
| Optional Sentry               | ✅     | Enabled only with `CONTEXTURA_SENTRY_DSN`                                                                 |

### Still pending

| Area                                 | Status | Notes                                                             |
| ------------------------------------ | ------ | ----------------------------------------------------------------- |
| Manual end-to-end smoke verification | [-]    | Still required after OCR helper stabilization                     |
| Valid OCR regression corpus          | [ ]    | `test-corpus/*.png` files are currently empty placeholders        |
| Updater signing pubkey               | [ ]    | `tauri.conf.json` still has an empty updater pubkey               |
| Quality-tier policy + RAM gate       | [ ]    | Model switching exists, but no curated tier policy or memory gate |
| Multi-display support                | [ ]    | Single-display focus only                                         |

## Non-Negotiable Model Constraint

The translation runtime is `llama-server`, so the active model must be a **decoder-only** GGUF model.

Supported family for the default setup:

- `Qwen3-0.6B Q4_K_M`

Unsupported in this architecture:

- NLLB
- MarianMT
- T5
- BART
- other encoder-decoder models

## Runtime Contracts

### Capture

- Source: ScreenCaptureKit stream
- Format: `PixelFormat::BGRA`
- Output: `CaptureFrame { data, width, height, display_id, scale_factor }`
- Scale factor: derived from the display’s pixel width divided by its point-space frame width
- Exclusion: Contextura’s own app windows are excluded from display capture
- Force scan: manual requests run against the latest cached frame if one is available
- Recovery: a stalled capture stream causes the runtime to rebuild the stream automatically

### Motion Gate

- Downsample source frames to `160x90` grayscale
- Compare active region only, excluding edge inset
- Feed motion ratio into `DebounceStateMachine`
- Trigger OCR only when the screen has settled past the configured debounce duration

### Snapshotting

- Temp file: `/tmp/contextura-frame-{frame_id}.png`
- Persistent debug file: `/tmp/contextura-frame-latest.png`
- Channel order: BGRA input converted to RGBA before PNG encoding

### OCR

- Binary: bundled `vision-helper`
- Input: PNG path
- Output: JSON array of text boxes with normalized Vision coordinates
- Failure mode: non-zero helper exit is treated as an OCR error, not as an empty OCR result
- Post-processing: coordinate conversion, vertical text handling, furigana suppression, confidence filtering, CJK filtering, IoU merge

### Translation

- Binary: bundled `llama-server`
- Bind address: `127.0.0.1:8765`
- Required launch arg: `--jinja`
- Required system prompt suffix: `/no_think`
- Strategy: numbered batched translation with rolling context memory

### Overlay IPC

| Event                 | Purpose                                                                    |
| --------------------- | -------------------------------------------------------------------------- |
| `translation-started` | show loading state                                                         |
| `translation-update`  | render styled translation boxes                                            |
| `translation-clear`   | clear stale overlay content                                                |
| `translation-error`   | report watchdog restart or runtime errors with title/detail/level metadata |

## Module Responsibilities

| File                           | Responsibility                                                   |
| ------------------------------ | ---------------------------------------------------------------- |
| `src-tauri/src/lib.rs`         | orchestration, setup, main runtime loop                          |
| `src-tauri/src/models.rs`      | model manifest loading, active-model resolution, model switching |
| `src-tauri/src/capture.rs`     | ScreenCaptureKit capture and frame extraction                    |
| `src-tauri/src/motion.rs`      | motion detection and debounce                                    |
| `src-tauri/src/ocr.rs`         | OCR subprocess and post-processing                               |
| `src-tauri/src/translation.rs` | sidecar start, health polling, batching, memory                  |
| `src-tauri/src/styling.rs`     | contrast-aware overlay styling                                   |
| `src-tauri/src/context.rs`     | app-switch invalidation                                          |
| `src-tauri/src/thermal.rs`     | thermal and battery throttling signals                           |
| `src-tauri/src/hotkeys.rs`     | global shortcuts                                                 |
| `src-tauri/src/tray.rs`        | tray menu behavior                                               |
| `src-tauri/src/ipc.rs`         | payload types sent to frontend                                   |
| `src/overlay.js`               | frontend event handling and box rendering                        |

## Verification Expectations

Rust verification is necessary but not sufficient. A feature is only operationally verified when the app is run with:

1. Screen Recording permission granted
2. A valid local Qwen3 GGUF model present
3. A successful live translation pass over real Japanese content

Those manual checks remain the next required validation step. The OCR helper runtime defect is fixed in this workspace. The latest code changes also patched force-scan behavior and strengthened overlay exclusion, but both still need live verification alongside end-to-end translation with a healthy local sidecar and real corpus assets.
