# AGENT.md — Instructions for Coding Agents

Read this file before changing code or project docs.

## Project

- Name: Contextura
- Purpose: real-time Japanese to English screen translation overlay for macOS
- Stack: Rust 2024, Tauri v2, ScreenCaptureKit, Swift `vision-helper`, `llama-server`, vanilla HTML/CSS/JS
- Platform: macOS 13+ on Apple Silicon

## Current Status

- The main capture → motion → OCR → translation → styling → IPC pipeline is wired in `src-tauri/src/lib.rs`.
- `Cmd+Shift+T` toggles the overlay.
- `Cmd+Shift+R` forces an OCR/translation pass by bypassing debounce.
- `Cmd+Shift+M` clears translation memory.
- The watchdog restarts `llama-server` after repeated health-check failures.
- Capture now requests BGRA explicitly and uses the display’s real scale factor.
- Battery detection uses `pmset -g batt`.
- Sentry is optional and enabled only when `CONTEXTURA_SENTRY_DSN` is set.

## Still Open

- End-to-end manual smoke verification is still required after major pipeline changes.
- `Cmd+Shift+G` remains a stub until model tier switching exists.
- Wizard screen 1 exists; screens 2–4 are not implemented.
- `downloader.rs`, richer CLI flows, curated `test-corpus/`, updater signing, and multi-display support are still future work.
- Overlay exclusion from capture (`excludedWindows`) is still pending.

## Source of Truth

- `TODO.md`: implementation tracker and remaining work
- `SPEC.md`: current technical contract and status
- `ARCHITECTURE.md`: module/data-flow overview
- `README.md`: user-facing setup and usage

If any of those files disagree with the code, update the docs in the same change.

## Coding Rules

- Keep application orchestration in `src-tauri/src/lib.rs`.
- Keep subsystem responsibilities split by file; do not merge modules casually.
- Prefer fixing stale docs rather than preserving inaccurate historical claims.
- Use `anyhow::Result` in app code and avoid `unwrap()` on production paths.
- Preserve Tauri v2 patterns: commands registered in `generate_handler!`, shell capabilities declared explicitly, `main.rs` thin.
- Prefer local, offline-friendly behavior. Do not introduce cloud dependencies for core translation flow.
- Frontend remains vanilla JS with no build step.

## Verification Rules

- At minimum, run `cargo test --manifest-path src-tauri/Cargo.toml`.
- Prefer also running `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings` after Rust changes.
- Do not mark manual runtime checks as done unless you actually performed them.

## Avoid

- Do not reintroduce encoder-decoder models such as NLLB, MarianMT, T5, or BART into the llama.cpp path.
- Do not add broad `#[allow(...)]` attributes to hide real issues.
- Do not claim a feature is “working properly” if only the code path exists but no manual runtime check was performed.
