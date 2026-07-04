# Contextura - AI Prompt Instructions

You are analyzing the **Contextura** codebase, a native macOS desktop application that captures the screen, performs OCR on Japanese text, translates it locally using `llama-server`, and renders translations as HTML overlay boxes over the original text.

## Project Stack & Architecture

- **Backend**: Rust, Tauri v2.
  - Native macOS screen capture uses Apple's **ScreenCaptureKit** framework.
  - OCR uses Apple's **Vision** framework via a Swift-based helper sidecar.
  - Local translation is managed via a `llama-server` sidecar, defaulting to the **TranslateGemma** model.
  - A thermal/battery watchdog and IPC handlers manage local server lifecycles, health checks, and context invalidation (e.g. clearing translation memory on active application switch).
- **Frontend**: Vanilla HTML5, CSS3, and JavaScript.
  - Key views: `index.html` (the primary overlay window), `wizard.html` (the onboarding wizard), `help.html` (help screen).
  - All communication is routed via Tauri IPC bindings (`invoke`, `emit`, `listen`).

## Codebase Patterns & Rules

1. **State Management**:
   - Rust backend state is managed using Tauri's state injection (`tauri::State`).
   - Frontend is event-driven vanilla JS. Do not introduce heavy framework code (e.g., React, Vue) unless explicitly instructed.
2. **Performance & Resources**:
   - Screen capture and OCR are CPU/GPU-heavy. Maintain strict frame debouncing (minimum 200ms settling phase) and screen-state motion tracking.
   - Prevent model memory bloat by invalidating history when changing target screens or foreground applications.
3. **Security Constraints**:
   - The overlay window must be configured as non-shareable (`NSWindowSharingType::None` via AppKit) to avoid self-capture loops.
   - Do not store sensitive/active frames in standard `/tmp/` directories in production; use private application cache paths.
4. **Testing**:
   - Unit tests are located in `src-tauri/src/`.
   - The CLI/test command pathways are routed through `cli.rs` and can be invoked using `--debug-cli`.
