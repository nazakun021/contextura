# SPEC.md — Contextura

**Version:** 1.6.0  
**Last Updated:** 2026-04-23  
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
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings` passes

### Code-integrated features

| Area | Status | Notes |
| --- | --- | --- |
| Tauri app bootstrap | ✅ | `main.rs` stays thin; `lib.rs` owns runtime |
| Screen capture | ✅ | Single display, explicit BGRA |
| Motion detection and debounce | ✅ | Wired into frame loop |
| PNG snapshot writing | ✅ | Temp file plus persistent latest debug copy |
| OCR subprocess | ✅ | Live path through `vision-helper` |
| Translation sidecar | ✅ | Qwen3-compatible args, health check, restart support |
| Dynamic styling | ✅ | WCAG-based foreground/background selection |
| IPC to overlay | ✅ | `translation-started`, `translation-update`, `translation-clear`, `translation-error` |
| Overlay toggle hotkey | ✅ | `Cmd+Shift+T` |
| Force OCR hotkey | ✅ | `Cmd+Shift+R` |
| Manual memory reset | ✅ | `Cmd+Shift+M` |
| Tray primary actions | ✅ | Toggle, translate now, clear context |
| Context invalidation | ✅ | App switch clears memory and overlay |
| Watchdog | ✅ | Restarts sidecar after repeated health failures |
| Thermal + battery awareness | ✅ | Thermal API + `pmset -g batt` |
| Optional Sentry | ✅ | Enabled only with `CONTEXTURA_SENTRY_DSN` |

### Still pending

| Area | Status | Notes |
| --- | --- | --- |
| Manual end-to-end smoke verification | [-] | Not re-run in this workspace |
| Overlay exclusion from capture | [ ] | Prevent self-capture loops |
| Model tier switching | [ ] | `Cmd+Shift+G` still stubbed |
| Wizard screens 2–4 | [ ] | Only initial permission screen exists |
| Real CLI E2E/test corpus flow | [ ] | Current CLI remains limited |
| Multi-display support | [ ] | Single-display focus only |

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
- Post-processing: coordinate conversion, vertical text handling, furigana suppression, confidence filtering, CJK filtering, IoU merge

### Translation

- Binary: bundled `llama-server`
- Bind address: `127.0.0.1:8765`
- Required launch arg: `--jinja`
- Required system prompt suffix: `/no_think`
- Strategy: numbered batched translation with rolling context memory

### Overlay IPC

| Event | Purpose |
| --- | --- |
| `translation-started` | show loading state |
| `translation-update` | render styled translation boxes |
| `translation-clear` | clear stale overlay content |
| `translation-error` | report watchdog restart or runtime errors |

## Module Responsibilities

| File | Responsibility |
| --- | --- |
| `src-tauri/src/lib.rs` | orchestration, setup, main runtime loop |
| `src-tauri/src/capture.rs` | ScreenCaptureKit capture and frame extraction |
| `src-tauri/src/motion.rs` | motion detection and debounce |
| `src-tauri/src/ocr.rs` | OCR subprocess and post-processing |
| `src-tauri/src/translation.rs` | sidecar start, health polling, batching, memory |
| `src-tauri/src/styling.rs` | contrast-aware overlay styling |
| `src-tauri/src/context.rs` | app-switch invalidation |
| `src-tauri/src/thermal.rs` | thermal and battery throttling signals |
| `src-tauri/src/hotkeys.rs` | global shortcuts |
| `src-tauri/src/tray.rs` | tray menu behavior |
| `src-tauri/src/ipc.rs` | payload types sent to frontend |
| `src/overlay.js` | frontend event handling and box rendering |

## Verification Expectations

Rust verification is necessary but not sufficient. A feature is only operationally verified when the app is run with:

1. Screen Recording permission granted
2. A valid local Qwen3 GGUF model present
3. A successful live translation pass over real Japanese content

Those manual checks remain the next required validation step.
