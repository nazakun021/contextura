# AGENT.md — Instructions for Coding Agents

Read this file before changing code or project docs.

## Project

- Name: Contextura
- Purpose: real-time Japanese to English screen translation overlay for macOS
- Stack: Rust 2024, Tauri v2, ScreenCaptureKit, Swift `vision-helper`, `llama-server`, vanilla HTML/CSS/JS
- Platform: macOS 13+ on Apple Silicon

## Current Status

- The main capture → motion → OCR → translation → styling → IPC pipeline is wired in `src-tauri/src/lib.rs`.
- The default local model path is now `translategemma-4b-it.Q4_K_M`, with Qwen-style GGUF models still supported as alternates.
- `Cmd+Shift+T` toggles the overlay.
- `Cmd+Shift+R` forces an OCR/translation pass by bypassing debounce.
- `Cmd+Shift+M` clears translation memory.
- `Cmd+Shift+G` cycles to the next installed local GGUF model and restarts the runtime.
- The watchdog restarts `llama-server` after repeated health-check failures.
- Capture now requests BGRA explicitly and uses the display’s real scale factor.
- Capture excludes the app’s own windows and restarts after prolonged frame stalls.
- Battery detection uses `pmset -g batt`.
- Sentry is optional and enabled only when `CONTEXTURA_SENTRY_DSN` is set.
- Wizard screens 1–4 now exist in `src/wizard.html`.
- `--debug-cli --input <png>` and `--test-suite <dir>` run the real OCR/translation path.
- `ocr.rs` now treats helper process failure as a real OCR error instead of silent empty output.
- The downloader (`downloader.rs`) is present, and model cycling, watchdog protection, secure cache snapshots, and all core CLI features are fully implemented.

## Source of Truth

- `SPEC.md`: current technical contract and status
- `README.md`: user-facing setup and usage
- `MISSION.md`: project tenets, offline and telemetry preferences
- `ROADMAP.md`: current and future phases
- `TECH-STACK.md`: tech components, capture, model requirements, and data flow

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
- If OCR or translation behavior is relevant, prefer the checks documented in `TEST.md`, including the debug CLI path and one live GUI smoke pass.
- Do not mark manual runtime checks as done unless you actually performed them.

## Avoid

- Do not reintroduce encoder-decoder models such as NLLB, MarianMT, T5, or BART into the llama.cpp path.
- Do not add broad `#[allow(...)]` attributes to hide real issues.
- Do not claim a feature is “working properly” if only the code path exists but no manual runtime check was performed.

## Agent skills

### Issue tracker

Issues and PRDs for this repo live as GitHub issues. External PRs are not treated as a request surface. See `docs/agents/issue-tracker.md`.

### Triage labels

Using standard triage labels (`needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`). See `docs/agents/triage-labels.md`.

### Domain docs

Single-context layout: `CONTEXT.md` at root, ADRs under `docs/adr/`. See `docs/agents/domain.md`.
