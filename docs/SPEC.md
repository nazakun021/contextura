# SPEC.md — Contextura

**Version:** 2.3.0  
**Last Updated:** 2026-07-19  
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
- `./scripts/smoke-wire-to-wire.sh --quick` passes
- Standalone `vision-helper` now fails fast on empty/corrupt input instead of returning a misleading empty result

### Code-integrated features

| Area                              | Status | Notes                                                                                                                                   |
| --------------------------------- | ------ | --------------------------------------------------------------------------------------------------------------------------------------- |
| Tauri app bootstrap               | ✅     | `main.rs` stays thin; `lib.rs` owns runtime bootstrap and command registration                                                          |
| Screen capture                    | ✅     | Single display, explicit BGRA; capture excludes own windows                                                                             |
| Motion detection and debounce     | ✅     | xxHash-based thumbnail deduplication; DebounceStateMachine wired into frame loop                                                        |
| Unified RGBA conversion           | ✅     | BGRA→RGBA swap happens once at capture boundary; downstream modules consume unified RGBA                                                |
| In-memory snapshot encoding       | ✅     | Runtime encodes PNG payloads in memory for OCR stdin transport; no file-path dependency in OCR hot path                                 |
| OCR subprocess                    | ✅     | Bundled `vision-helper` validates input frames and returns OCR JSON; backend transport and text post-processing are now split           |
| Pluggable translation strategy    | ✅     | TranslateGemma and Qwen strategies are pluggable; new strategies don't require touching client internals                                |
| Translation sidecar               | ✅     | Health check, startup, readiness, and watchdog recovery now route through a dedicated Sidecar lifecycle seam                            |
| Concurrent pipeline               | ✅     | Styling color-sampling and LLM translation run concurrently; results merged before overlay update                                       |
| Dynamic styling                   | ✅     | WCAG-based foreground/background selection from RGBA pixels                                                                             |
| IPC to overlay                    | ✅     | `translation-started`, `translation-update`, `translation-clear`, `translation-error`; started-phase payload now includes pending boxes |
| Fail-loud error UI                | ✅     | Persistent error card rendered in overlay on `translation-error`; prompts manual retry                                                  |
| Smart overlay presentation        | ✅     | CSS fade-in/out transitions, skeleton loaders, horizontal/vertical collision avoidance                                                  |
| Event-driven capture loop         | ✅     | `tokio::select!` over frame channel, command channel, and async debounce timer                                                          |
| Deterministic settings reload     | ✅     | 60-second timer removed; settings reload immediately on pipeline commands                                                               |
| Overlay toggle hotkey             | ✅     | `Cmd+Shift+T`                                                                                                                           |
| Force OCR hotkey                  | ✅     | `Cmd+Shift+R`; bypasses debounce and runs against latest cached frame                                                                   |
| Manual memory reset               | ✅     | `Cmd+Shift+M`                                                                                                                           |
| Model cycling hotkey              | ✅     | `Cmd+Shift+G` cycles to next installed local model and restarts the runtime                                                             |
| Tray primary actions              | ✅     | Toggle, translate now, clear context                                                                                                    |
| Context invalidation              | ✅     | App switch clears memory and overlay                                                                                                    |
| Watchdog                          | ✅     | Restarts sidecar after 3 consecutive health failures                                                                                    |
| Overlay capture exclusion         | ✅     | Excludes own windows from capture; overlay marked `NSWindowSharingType::None`                                                           |
| Wizard screens 1–4                | ✅     | Setup flow covers permissions, model, controls, ready state                                                                             |
| Real CLI OCR/translation path     | ✅     | Code path is live and end-to-end verified using local LLM sidecar                                                                       |
| Golden-file integration runner    | ✅     | `--test-suite` flag runs corpus assertions; `evaluate_corpus_case` unit-tested; Rust test suite currently reports 135 passing tests     |
| Capture restart handling          | ✅     | Stalled capture stream triggers rebuild                                                                                                 |
| Thermal + battery awareness       | ✅     | Thermal API + `pmset -g batt`                                                                                                           |
| Optional Sentry                   | ✅     | Enabled only with `CONTEXTURA_SENTRY_DSN`                                                                                               |
| Updater signing pubkey support    | ⚠️     | Updater endpoints are configured, but `plugins.updater.pubkey` is currently empty in `tauri.conf.json`                                  |
| Quality-tier policy + model cycle | ✅     | Model switching and tier categorization (Standard/Quality/Custom) are fully implemented in `models.rs`                                  |
| Single-display capture            | ✅     | Core display capture and targeting is fully implemented and verified                                                                    |
| ocr_boxes golden tests            | ✅     | Integration testing framework supports coordinate checking; `test-corpus` fixtures are active in the `--test-suite` path                |
| Runtime latency instrumentation   | ✅     | Pipeline and translation stages emit `[Latency]` debug logs for OCR, concurrent stage, and chat completion timing                       |
| Wire-to-wire smoke harness        | ✅     | `scripts/smoke-wire-to-wire.sh` automates compile/test gates and `--debug-cli` OCR→translation verification against `test-corpus`       |

### Still pending

| Area                                 | Status | Notes                                                              |
| ------------------------------------ | ------ | ------------------------------------------------------------------ |
| Manual end-to-end smoke verification | [-]    | Required with a valid local model and real Japanese screen content |
| Updater signing key injection        | [-]    | Set `plugins.updater.pubkey` before production release             |
| Frontend CSP hardening               | [-]    | `app.security.csp` is currently null in Tauri config               |

## Non-Negotiable Model Constraint

The translation runtime is `llama-server`, so the active model must be a **decoder-only** GGUF model.

Supported family for the default setup:

- `TranslateGemma 4B IT Q4_K_M`
- Qwen-style decoder-only GGUF models

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
- Exclusion: Contextura’s own app windows are excluded from display capture, and the overlay window is marked non-shareable through AppKit
- Force scan: manual requests run against the latest cached frame if one is available
- Recovery: a stalled capture stream causes the runtime to rebuild the stream automatically

### Motion Gate

- Downsample source frames to `160x90` grayscale
- Compare active region only, excluding edge inset
- Feed motion ratio into `DebounceStateMachine`
- Trigger OCR only when the screen has settled past the configured debounce duration
- Default debounce is `200ms`
- Settling ignores low-level residual motion unless it exceeds `motion_threshold * 3.0`

### Snapshotting

- Channel order: BGRA input converted to RGBA **once** at the capture boundary; downstream modules consume RGBA
- OCR transport: RGBA frames are PNG-encoded in memory and streamed to `vision-helper` over stdin
- Snapshot files are no longer required in the OCR hot path

### OCR

- Binary: bundled `vision-helper`
- Input: stdin PNG bytes (`--stdin`) in runtime path; PNG file path remains supported for compatibility
- Output: JSON array of text boxes with normalized Vision coordinates
- Failure mode: missing, empty, corrupt, timed-out, or non-zero helper runs are treated as OCR errors, not as empty OCR results
- Module split: `ocr_backend.rs` owns helper execution and transport; `ocr_post_processor.rs` owns coordinate conversion, filtering, and deduplication
- Candidate selection: helper inspects multiple Vision candidates per observation and favors Japanese/CJK text when available
- Post-processing: coordinate conversion, text normalization, reading-order sort, furigana suppression, confidence filtering, CJK filtering, duplicate suppression for near-identical detections

### Translation

- Binary: bundled `llama-server`
- Bind address: `127.0.0.1:8765`
- Required launch arg by strategy: `--no-jinja` for TranslateGemma; `--jinja` for Qwen/LFM
- TranslateGemma strategy: sequential structured chat requests per string within each chunk
- Qwen strategy: numbered batched translation with rolling context memory and `/no_think`
- Active strategy is selected from the active model ID

### Overlay IPC

| Event                 | Purpose                                                                    |
| --------------------- | -------------------------------------------------------------------------- |
| `translation-started` | show loading state with pending box geometry and source text metadata      |
| `translation-update`  | render styled translation boxes                                            |
| `translation-clear`   | clear stale overlay content                                                |
| `translation-error`   | report watchdog restart or runtime errors with title/detail/level metadata |

## Module Responsibilities

| File                                       | Responsibility                                                                                   |
| ------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| `src-tauri/src/lib.rs`                     | Orchestration, Tauri bootstrap, command registration; thin runtime shell                         |
| `src-tauri/src/scheduler.rs`               | Event-driven pipeline loop (`tokio::select!`), debounce, concurrent styling+translation dispatch |
| `src-tauri/src/models.rs`                  | Model manifest loading, active-model resolution, model switching                                 |
| `src-tauri/src/capture.rs`                 | ScreenCaptureKit capture, BGRA→RGBA unified conversion at capture boundary                       |
| `src-tauri/src/motion.rs`                  | xxHash-based thumbnail deduplication, DebounceStateMachine                                       |
| `src-tauri/src/snapshot.rs`                | In-memory PNG encoder, stale temp-frame cleanup helpers, BGRA→RGBA swap helper                   |
| `src-tauri/src/ocr.rs`                     | OCR facade over backend acquisition and text post-processing                                     |
| `src-tauri/src/ocr_backend.rs`             | Vision helper adapter, stdin PNG handoff, timeout/kill, JSON decode                              |
| `src-tauri/src/ocr_post_processor.rs`      | Coordinate conversion, furigana suppression, script filtering, reading order, dedup              |
| `src-tauri/src/translation.rs`             | Pluggable strategy dispatch, batching, memory, watchdog owner                                    |
| `src-tauri/src/sidecar_runtime_adapter.rs` | Sidecar lifecycle seam: start, ready/retry wait, runtime-ready, recovery                         |
| `src-tauri/src/runtime_executor.rs`        | Thin runtime shell over Sidecar readiness execution                                              |
| `src-tauri/src/styling.rs`                 | Contrast-aware overlay styling from RGBA pixels                                                  |
| `src-tauri/src/ipc.rs`                     | Payload types for all frontend IPC events including `TranslationErrorPayload`                    |
| `src-tauri/src/context.rs`                 | App-switch invalidation                                                                          |
| `src-tauri/src/thermal.rs`                 | Thermal and battery throttling signals                                                           |
| `src-tauri/src/hotkeys.rs`                 | Global shortcuts                                                                                 |
| `src-tauri/src/tray.rs`                    | Tray menu behavior                                                                               |
| `src-tauri/src/path_resolver.rs`           | Binary path resolution, available-port discovery                                                 |
| `src-tauri/src/cli.rs`                     | Debug-CLI and `--test-suite` golden-file runner                                                  |
| `src/overlay.js`                           | Frontend event handling, collision avoidance, fade transitions, skeleton loaders, error card     |

## Verification Expectations

Rust verification is necessary but not sufficient. A feature is only operationally verified when the app is run with:

1. Screen Recording permission granted
2. A valid local decoder-only GGUF model present
3. A successful live translation pass over real Japanese content

The Daily-Driver Hardening PRD (issue #1) is complete. All 10 sub-issues (#2–#11) are closed. The Rust suite currently reports 135 passing tests across core subsystems. Manual end-to-end smoke verification with a live model remains the next required validation step.
