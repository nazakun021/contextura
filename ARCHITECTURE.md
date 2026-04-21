# Contextura Architecture

This document describes the overall architecture and current project status of the Contextura real-time screen translation overlay.

## Overall Architecture

The application comprises a Tauri v2 wrapper acting as the primary host. The application pipeline revolves around orchestrating continuous screen captures into a local translation process.

### 1. Frontend (Tauri WebView)

- **Tech Stack:** Vanilla HTML/CSS/JS.
- **Responsibility:** A minimal, framework-less overlay that is fully transparent and click-through. It receives JSON IPC payloads containing translated texts and their bounding boxes and dynamically renders them above the original content.

### 2. Backend (Rust)

The core logic resides in Rust and runs completely offline once models are downloaded:

- **`screencapturekit` integration:** Safely interfaces with Apple's ScreenCaptureKit APIs to grab continuous frames (up to 30 FPS).
- **Motion Detection (`motion.rs`):** Computes pixel differences between frames and extracts the largest contiguous diff using a 4-connected flood-fill algorithm. This prevents redundant OCR requests from running while the user is actively scrolling, drastically reducing thermal load.
- **OCR Engine (`ocr.rs`):** Triggers `vision-helper` upon motion debouncing. Coordinates mapping from the Vision framework are transformed into application logic bounds. Suppresses furigana annotations using a heuristic based on bounding box overlaps and relative heights.
- **Translation Engine (`translation.rs`):** A client for a bundled `llama-server` binary, handling requests and applying translation memory. Uses HTTP to communicate with the sidecar process on port 8765.

### 3. Sidecars & Helpers

- **`vision-helper`:** A lightweight Swift CLI tool that utilizes Apple's `Vision` framework to recognize Japanese text. Emits coordinate and confidence JSON arrays to `stdout`.
- **`llama-server`:** A bundled pre-compiled executable from the `llama.cpp` project. Employs `Metal` hardware acceleration on Apple Silicon to serve a fast translation endpoint natively, bypassing complex FFI setups.

## Current Project Status

**The project has completed its "Pipeline Activation" (Phase P).**

- **Implemented:**
  - Full initialization of the transparent Tauri overlay with dynamic window rendering properties.
  - Native ScreenCaptureKit frame capture bindings and output handler parsing.
  - Swift-based Vision text extraction subprocess implementation.
  - Llama.cpp server sidecar download, bundling, configuration, and HTTP interaction.
  - Dynamic WCAG 2.1 compliant box styling using `rayon` for parallel color sampling.
  - Global macOS hotkeys, system tray menu integration, and initial settings file.

- **Remaining / Focus Areas (Phase 8 and beyond):**
  - Apple Developer Code Signing and Notarization.
  - App packaging (`.dmg`) and releasing.
  - Performance profiling (`cargo flamegraph` / Xcode Instruments) and potential pre-allocation of buffers in SCKit output handling.
  - Comprehensive Edge Case E2E testing framework.

## Data Flow Summary

1. `SCStream` continuously outputs pixel buffers.
2. `MotionDetector` downsizes frames to double-buffered grayscale maps and calculates delta.
3. If delta is negligible for `DEBOUNCE_MS` (e.g., 300ms), a trigger fires.
4. The frame is encoded to PNG and passed to `vision-helper`.
5. OCR results are filtered, furigana suppressed, and forwarded to `llama-server`.
6. Previous context entries are appended to the batch request context slice to enhance inference quality.
7. Post-translation bounding box styling is processed in parallel.
8. Serialized objects emit via `tauri::Emitter` to the frontend DOM.
