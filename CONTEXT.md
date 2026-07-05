# CONTEXT.md — Contextura Ubiquitous Language

> This glossary defines the canonical terms used throughout the Contextura codebase, docs, and conversations.
> When naming code, writing tests, filing issues, or proposing changes — **use these terms exactly**.
> If a concept isn't here, either it doesn't exist yet in the domain, or it needs to be added.

---

## Pipeline Stages

| Term                  | Definition                                                                                                                                                                              | Avoid                                                       |
| :-------------------- | :-------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | :---------------------------------------------------------- |
| **Capture Frame**     | A raw pixel buffer (`CaptureFrame`) received from ScreenCaptureKit. Always BGRA format with metadata (width, height, display_id, scale_factor).                                         | "screenshot", "image", "bitmap"                             |
| **Motion Gate**       | The subsystem that downsamples frames to 160×90 grayscale thumbnails and computes a motion ratio to decide whether the screen content has changed.                                      | "motion detector" (acceptable as code name), "diff checker" |
| **Debounce**          | A state machine (`DebounceStateMachine`) that suppresses OCR triggers during active scrolling and fires only after the screen has settled for a configured duration (default 200ms).    | "throttle", "rate limiter"                                  |
| **Snapshot**          | The act of converting a BGRA capture frame to RGBA and encoding it as a PNG file for the OCR subprocess. The pipeline writes numbered snapshots and a persistent `latest` debug copy.   | "save", "dump"                                              |
| **OCR Pass**          | A single invocation of the `vision-helper` subprocess on a snapshot PNG. Returns JSON text boxes with normalized Vision coordinates.                                                    | "text recognition", "scan" (acceptable informally)          |
| **Translation Batch** | A set of OCR-extracted text strings sent to the translation sidecar for LLM inference. The batching strategy varies by model family (sequential for TranslateGemma, numbered for Qwen). | "prompt", "inference request"                               |
| **Styled Box**        | A `TranslationBox` combining translated text, bounding coordinates, and WCAG-compliant foreground/background colors sampled from the frame's RGBA buffer.                               | "overlay item", "text block"                                |
| **IPC Event**         | A Tauri event (`translation-started`, `translation-update`, `translation-clear`, `translation-error`) emitted from the Rust backend to the overlay frontend.                            | "message", "notification"                                   |

## Runtime Components

| Term                     | Definition                                                                                                                                                                                             | Avoid                                        |
| :----------------------- | :----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | :------------------------------------------- |
| **Orchestrator**         | The main runtime coordination logic in `lib.rs`. Owns the pipeline loop, subsystem initialization, and Tauri app setup.                                                                                | "controller", "manager"                      |
| **Sidecar**              | A managed child process. Contextura has two: `vision-helper` (OCR) and `llama-server` (translation). Each has its own lifecycle, health checks, and restart logic.                                     | "subprocess" (acceptable in code), "service" |
| **Translation Client**   | The Rust-side abstraction (`TranslationClient`) that manages the `llama-server` lifecycle, health polling, and request dispatch.                                                                       | "LLM client", "inference engine"             |
| **Watchdog**             | An async task that periodically health-checks the translation sidecar and restarts it after 3 consecutive failures.                                                                                    | "monitor", "health checker"                  |
| **Translation Strategy** | The model-specific prompting and batching logic used to format requests for the active LLM. Currently two: `TranslateGemma` (sequential structured chat) and `Qwen` (numbered batch with `/no_think`). | "prompt template", "model adapter"           |
| **Pipeline Command**     | An enum (`PipelineCommand`) representing control signals: `ForceScan`, `ReloadRuntime`, `Shutdown`. Sent via crossbeam channel.                                                                        | "event", "action"                            |
| **Runtime State**        | The loaded combination of user settings and active model status, refreshed periodically or on explicit reload.                                                                                         | "config", "app state"                        |

## Frontend

| Term        | Definition                                                                                                   | Avoid                        |
| :---------- | :----------------------------------------------------------------------------------------------------------- | :--------------------------- |
| **Overlay** | The transparent Tauri WebView window that renders styled translation boxes over the user's screen content.   | "HUD", "popup"               |
| **Wizard**  | A 4-step first-run setup flow covering Screen Recording permission, model placement, hotkeys, and readiness. | "onboarding", "setup dialog" |

## Model Domain

| Term                   | Definition                                                                                                                                                                       | Avoid                                  |
| :--------------------- | :------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | :------------------------------------- |
| **Decoder-Only Model** | The only model architecture supported by `llama-server`. TranslateGemma and Qwen are decoder-only. Encoder-decoder models (NLLB, MarianMT, T5, BART) are explicitly unsupported. | "LLM" (too vague), "transformer model" |
| **Model Manifest**     | The registry of known model families, their expected filenames, and strategy mappings. Defined in `models.rs`.                                                                   | "model config", "model list"           |
| **Active Model**       | The currently selected GGUF model that `llama-server` will load. Determined by settings + manifest resolution.                                                                   | "current model", "loaded model"        |

## Platform

| Term                     | Definition                                                                                                                                                                                       | Avoid                     |
| :----------------------- | :----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | :------------------------ |
| **Scale Factor**         | The ratio of physical pixels to logical points, derived dynamically from ScreenCaptureKit display metadata. Used for coordinate conversion between Vision (normalized) and overlay (CSS pixels). | "retina scale", "DPI"     |
| **Capture Exclusion**    | The mechanism that prevents the overlay window from being captured in its own screenshots. Uses both ScreenCaptureKit window filtering and AppKit `NSWindowSharingType::None`.                   | "self-capture prevention" |
| **Context Invalidation** | Clearing the translation memory and overlay content when the user switches to a different application. Detected by `AppWindowTracker`.                                                           | "cache clear", "reset"    |
