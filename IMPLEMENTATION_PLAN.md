# Contextura Pipeline Implementation Plan

This plan details how we will transition the application from its current "scaffolded" state (where UI and structure exist but the core pipeline is mocked) into a fully functional real-time translation overlay.

## Background Context

Currently, the application runs and shows the onboarding wizard and system tray, but the core engine in `main.rs` is detached (`#[expect(dead_code...)]`) and the modules (`capture`, `motion`, `ocr`, `translation`) only contain boilerplate or mocks. To "make it translate now", we need to implement the actual underlying macOS and ML frameworks and wire them together.

## User Review Required

> [!IMPORTANT]
> Because writing raw Objective-C FFI bindings for `Vision` and `ScreenCaptureKit` in Rust can be highly unstable and error-prone, I propose we use small, bundled **Swift Subprocesses** for screen capture and OCR, or rely on stable wrapper crates if available.
>
> For **Translation**, the spec calls for compiling `llama.cpp` via rust bindings (`llama-cpp-rs`). Building `llama.cpp` with Metal support inside a Tauri Rust app takes significant setup. A highly stable alternative is to bundle the pre-compiled `llama-server` (from the llama.cpp project) and have Tauri manage its lifecycle as a sidecar process, communicating with it over a local HTTP port.
>
> **Please let me know your preferences on these architectural choices.**

## Proposed Changes

---

### Core Pipeline Loop

We need to orchestrate the flow of data continuously in the background.

#### [MODIFY] `src-tauri/src/main.rs`

- Remove `#[expect(dead_code)]` from the pipeline modules (`capture`, `motion`, `ocr`, `translation`).
- Within the `tauri::Builder::setup` closure, spawn a background orchestration thread.
- This thread will receive `CaptureFrame` structs, pass them through the `MotionDetector`, and on `DebounceEvent::Triggered`, send the frame to the `OcrEngine`.
- Finally, it will pass the OCR text to the `TranslationEngine` and invoke the Tauri IPC event `translation-update` to the frontend.

#### [MODIFY] `src-tauri/Cargo.toml`

- Add necessary dependencies such as `screencapturekit` (if using Rust for capture), `reqwest` (for sidecar communication), and serialization helpers.

---

### Phase 1: Real Screen Capture

#### [MODIFY] `src-tauri/src/capture.rs`

- Remove the `thread::spawn` mock.
- Implement true ScreenCaptureKit capture. If the `screencapturekit` crate bindings are insufficient, we will write a tiny Swift helper that utilizes `SCStream` and pipes raw pixel buffers to Rust.
- Connect the captured frames to the `crossbeam` channel for processing.

---

### Phase 2: Apple Vision OCR

#### [NEW] `src-tauri/src/bin/vision-helper.swift`

- Create the Swift CLI tool as defined in `SPEC.md`. This tool will use `VNRecognizeTextRequest` with `["ja-JP"]` to extract text, confidence, and bounding boxes, returning a JSON array to stdout.
- This entirely bypasses the need for unstable `objc2-vision` bindings.

#### [MODIFY] `src-tauri/src/ocr.rs`

- Update the `OcrEngine` to invoke `vision-helper.swift` (or its compiled binary) via `std::process::Command`, passing a temporary path to the saved frame thumbnail.
- Parse the JSON output into `OcrResult` structs and perform the furigana suppression logic.

---

### Phase 3: Translation Engine

#### [MODIFY] `src-tauri/src/translation.rs`

- Replace the mock uppercase translation with actual inference.
- Depending on your chosen approach (Rust bindings vs. `llama-server` sidecar), implement the loading of the `nllb-200-distilled-600M.Q4_K_M.gguf` model.
- Keep the `TranslationMemory` logic for context rolling, generating the full batched prompt.
- Handle fallback when the system is offline or encountering Metal errors.

## Open Questions

> [!WARNING]
>
> 1. **Model Management:** Do you already have the `nllb-200-distilled-600M.Q4_K_M.gguf` file locally, or should I also implement the Model Downloader (Phase 3.1) so the app fetches it automatically first?
> 2. **Sidecar vs Rust bindings:** Are you okay with using `llama-server` as a bundled sidecar for stability, instead of compiling `llama.cpp` directly into the Rust binary?
> 3. **Swift OCR Helper:** Are you comfortable with using a compiled Swift CLI tool for the OCR task as a fallback from raw `objc2`?

## Verification Plan

### Automated Tests

- Run `cargo run --bin contextura -- --debug-cli --test-suite ../test-corpus` to ensure the E2E mock pipeline parses JSON correctly (even if using swift/sidecars).

### Manual Verification

1. Launch the Tauri application.
2. Ensure no panic occurs on startup.
3. Open a Japanese textbook or manga snippet on screen.
4. Stop mouse/scroll movement; wait for the 300ms debounce.
5. Verify that translated overlay boxes appear successfully and align accurately with the original text.
