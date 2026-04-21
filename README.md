# Contextura

Contextura is a high-performance desktop overlay application for macOS (Apple Silicon native) that translates Japanese text visible on-screen to English in real-time. It uses local AI models to ensure everything runs 100% offline, keeping your screen contents secure while remaining thermally responsible through IOKit monitoring and motion-debounced inference.

## Features

- **Real-Time Overlay:** Absolutely positioned transparent translation boxes layered perfectly over native applications.
- **Offline First:** Apple Vision API for OCR and `llama.cpp` for translations right on your Mac.
- **Smart Debouncing:** Frame motion-detection algorithms prevent redundant OCR loops while you're scrolling.
- **Dynamic UX:** Hotkeys, System Menu Tray, First-Run Setup Wizard, and automated crash recovery.
- **Thermal Adaptive:** Automatically throttles frame processing when system thermal pressure rises to save battery.

## Current Implementation Status

**Phases 0 through 6 have been successfully scaffolded and implemented!**
- Phase 0: Project setup and CLI debug harness
- Phase 1: ScreenCaptureKit interfaces and connected-components motion detection
- Phase 2: Apple Vision OCR integration with furigana suppression
- Phase 3: Translation Memory and app context invalidation tracking
- Phase 4: Dynamic background color styling adhering to WCAG 2.1
- Phase 5: Tauri WebView front-end overlay and IPC payloads
- Phase 6: Global shortcuts, System Tray integration, and Wizards

## Architecture Setup
To review the detailed architecture and best practices applied during development, see [SPEC.md](./SPEC.md) and [TODO.md](./TODO.md).
