# Contextura

Contextura is a high-performance desktop overlay application for macOS (Apple Silicon native) that translates Japanese text visible on-screen to English in real-time. It uses local AI models to ensure everything runs 100% offline, keeping your screen contents secure while remaining thermally responsible through IOKit monitoring and motion-debounced inference.

## Features

- **Real-Time Overlay:** Absolutely positioned transparent translation boxes layered perfectly over native applications.
- **Offline First:** Apple Vision API for OCR and `llama.cpp` for translations right on your Mac.
- **Smart Debouncing:** Frame motion-detection algorithms prevent redundant OCR loops while you're scrolling.
- **Dynamic UX:** Hotkeys, System Menu Tray, First-Run Setup Wizard, and automated crash recovery.
- **Thermal Adaptive:** Automatically throttles frame processing when system thermal pressure rises to save battery.

## Current Implementation Status

**Phases 0 through 6 and Phase P have been successfully scaffolded and implemented!**

- Phase 0: Project setup and CLI debug harness
- Phase 1: ScreenCaptureKit capture + motion detection
- Phase 2: Apple Vision OCR integration with furigana suppression
- Phase 3: Translation Memory and app context invalidation tracking
- Phase 4: Dynamic background color styling adhering to WCAG 2.1
- Phase 5: Tauri WebView front-end overlay and IPC payloads
- Phase 6: Global shortcuts, System Tray integration, and Wizards
- Phase P: Pipeline Activation — wired real implementations for capture, OCR, and translation sidecar

**Next steps:**

- Phase 8: Code signing, notarization, distribution

## Development Setup

### Prerequisites

- macOS 13+ (Apple Silicon)
- [Rust Toolchain](https://rustup.rs/)

### Running Locally

To run the app in development mode with hot-reloading:

1. Clone the repository and cd into it.
2. The UI is vanilla HTML/JS located in `./src`, so no Node/npm build step is required!
3. Run the Tauri application from the `src-tauri` directory:
   ```bash
   cd src-tauri
   cargo run
   ```

To run the headless Debug CLI and execute the E2E test suite mock pipeline:

```bash
cd src-tauri
cargo run --bin contextura -- --debug-cli --test-suite ../test-corpus
```

## Usage

Once installed and running, Contextura sits quietly in your macOS menu bar.

1. **Start Reading**: Simply open any app containing Japanese text. Contextura detects when you stop scrolling or moving your mouse and automatically translates the text.
2. **Translation Overlay**: The English translations will appear directly over the Japanese text in a transparent, click-through overlay.
3. **Menu Bar Settings**: Click the Contextura icon in your menu bar (top right of your screen) to temporarily disable the overlay, switch models, clear context, or manage settings.

### Global Hotkeys

You can control the app seamlessly without clicking the menu bar using these global macOS shortcuts:

- <kbd>Cmd</kbd> + <kbd>Shift</kbd> + <kbd>T</kbd> : **Toggle Overlay** (Hide/Show translations)
- <kbd>Cmd</kbd> + <kbd>Shift</kbd> + <kbd>R</kbd> : **Force Translate** (Bypass scroll-debouncing and immediately translate the current screen)
- <kbd>Cmd</kbd> + <kbd>Shift</kbd> + <kbd>M</kbd> : **Clear Memory Context** (Manually clear previous pages' translations from the AI model's context)
- <kbd>Cmd</kbd> + <kbd>Shift</kbd> + <kbd>G</kbd> : **Switch Model** (Toggle between fast NLLB and high-quality Gemma 4, if device meets 12GB RAM requirements)
- <kbd>Cmd</kbd> + <kbd>Shift</kbd> + <kbd>Q</kbd> : **Quit Application**

## Architecture Setup

To review the detailed architecture and best practices applied during development, see [SPEC.md](./SPEC.md) and [TODO.md](./TODO.md).
