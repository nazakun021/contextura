# AGENT.md — Instructions for AI Coding Agents

> This file provides context and rules for any AI assistant (Claude, Copilot, Gemini, Cursor, etc.) working on this codebase. Read this file completely before making any changes.

---

## Project Identity

**Name:** Contextura  
**What it does:** Real-time Japanese → English screen translation overlay for macOS  
**Stack:** Rust + Tauri v2 · Swift `vision-helper` subprocess · `llama-server` sidecar (Qwen3-0.6B) · Vanilla HTML/CSS/JS  
**Platform:** macOS 13+ (Apple Silicon only)  
**Rust Edition:** 2024

---

## Current State (as of 2026-04-22)

**Still remaining:**

- `Cmd+Shift+T` (toggle overlay) — log stub
- `Cmd+Shift+R` (force OCR) — log stub
- Real `scale_factor` from display (hardcoded `2.0` in `capture.rs`)
- Watchdog health polling thread
- Wizard screens 2–4
- Battery check in `thermal.rs` (thermal reads correctly; battery hardcoded `false`)

**Model:** Qwen3-0.6B Q4_K_M (~350MB). Must be downloaded manually. NLLB is incompatible with llama-server (encoder-decoder architecture).

---

## Documentation Trust Level

- **AGENT.md** (this file) — Ground truth for agent behavior. Updated 2026-04-22
- **TODO.md** — Accurate. Trust `[x]`/`[-]`/`[ ]` markers
- **SPEC.md v1.5.0** — Audit-accurate, includes NLLB→Qwen3 migration rationale
- **ARCHITECTURE.md** — Current data flow and module reference
- **PRODUCTION.md v3.1.0** — Accurate status and priority ordering

> ⚠️ If SPEC.md or TODO.md says something is "✅ Done" but the compiler emits a `dead_code` warning for it, **the code is not actually used**. Trust compiler warnings over documentation.

> After Implementing items on SPEC.md or TODO.md **Update their files** to reflect the current state of the codebase.

---

## Architecture Quick Reference

```
src-tauri/src/
├── lib.rs            # App entry + full pipeline orchestration (THE FILE to edit)
├── main.rs           # Thin wrapper: calls app_lib::run()
├── capture.rs        # ScreenCaptureKit, display 0, BGRA frames
├── motion.rs         # MotionDetector (160×90) + DebounceStateMachine
├── ocr.rs            # vision-helper subprocess + coordinate conversion
├── translation.rs    # llama-server HTTP client + rolling context memory
├── context.rs        # NSWorkspace app-switch tracker (invalidation channel)
├── thermal.rs        # IOKit thermal state (battery hardcoded false)
├── styling.rs        # WCAG 2.1 luminance + sample_rect_ring()
├── ipc.rs            # TranslationBox / TranslationPayload (now emitted)
├── hotkeys.rs        # Global shortcuts (T, R are log stubs; Q, M work)
├── tray.rs           # System tray menu (most handlers log-only)
├── settings.rs       # settings.json read/write, defaults
├── downloader.rs     # Model downloader (never called, dead code)
└── cli.rs            # CLI arg parsing (outputs are stubs)

src-tauri/binaries/
├── llama-server-aarch64-apple-darwin    # llama.cpp server
├── vision-helper-aarch64-apple-darwin   # Swift OCR tool
└── lib*.dylib                           # llama.cpp Metal runtime

src/
├── index.html / overlay.js / overlay.css   # Overlay (all 4 IPC events handled)
├── wizard.html                              # First-run (screen 1 only)
└── help.html
```

### Key Qwen3-Specific Requirements

| Requirement                  | Where                                | Why                                                   |
| ---------------------------- | ------------------------------------ | ----------------------------------------------------- |
| `--jinja` launch flag        | `translation.rs` `start_sidecar()`   | Enables Qwen3's embedded Jinja2 chat template         |
| `/no_think` in system prompt | `translation.rs` `translate_batch()` | Disables thinking tokens that break `^(\d+):` parser  |
| Decoder-only `.gguf` model   | models dir                           | llama-server only supports decoder-only architectures |

### Key External Dependencies

| Crate                                          | Purpose                  | Notes                                  |
| ---------------------------------------------- | ------------------------ | -------------------------------------- |
| `screencapturekit`                             | Frame capture            | Real SCKit bindings                    |
| `objc2` / `objc2-foundation` / `objc2-app-kit` | macOS native APIs        | NSWorkspace, thermal                   |
| `crossbeam-channel`                            | Frame delivery           | Bounded channel, capacity 2            |
| `reqwest`                                      | HTTP to llama-server     | async, needs tokio                     |
| `tauri-plugin-shell`                           | Sidecar spawn            | Needs `shell:allow-execute` capability |
| `image`                                        | PNG encoding (BGRA→RGBA) | Used in `save_frame_as_png()`          |
| `rayon`                                        | Parallel styling         | `par_iter()` over OCR results          |

---

## Coding Rules

### Rust Style

- **Edition 2024** — use `let-else`, `let chains` in `if let` freely
- **Clippy:** `deny` on `all` and `perf`; `warn` on `pedantic`. The project enforces strict linting
- **Error handling:** Use `anyhow::Result` for application code, `thiserror` for library-style errors
- **No `unwrap()` in production paths.** Use `?`, `unwrap_or_default()`, or log + continue
- **Async:** Pipeline thread uses `tokio::runtime::Runtime::new().block_on()`. Translation calls are async
- **Preserve existing `#[allow(...)]` annotations** unless the code they suppress is being actively fixed

### Module Boundaries

- **Do not merge modules.** Each `.rs` file has a single responsibility
- **Do not add new modules** unless introducing a genuinely new subsystem
- **`lib.rs` is the orchestrator.** All wiring belongs there. Subsystem modules must not import each other (except `ipc.rs` structs)

### Frontend

- **Vanilla JS only.** No frameworks, no build step, no npm
- **No TypeScript.** The frontend is ~170 lines across 3 files
- **Tauri IPC** uses `window.__TAURI__.event.listen()` — all 4 events are already registered in `overlay.js`

### macOS-Specific

- **`objc2` bindings** — follow patterns in `context.rs` and `thermal.rs`
- **Entitlements:** `com.apple.security.screen-capture` is required and present
- **`macOSPrivateApi: true`** in `tauri.conf.json` enables transparent window background
- **Sidecar binaries** must include the target triple suffix (`llama-server-aarch64-apple-darwin`)

---

## Known Issues

### Capabilities

`shell:allow-execute` and `shell:allow-spawn` are now in `capabilities/default.json`. If you see the sidecar failing to spawn silently, verify these are still present — they can be accidentally removed.

### Model Architecture (CRITICAL)

The bundled `llama-server` supports **only decoder-only** transformer architectures. NLLB, T5, BART, and MarianMT are encoder-decoder and will fail immediately with:

```
llama_model_load: error loading model: unknown model architecture: 'nllb'
```

If you see this error, the wrong model file is being loaded. Check `manifest.json` → `filename` and verify the `.gguf` file exists at that path.

### Pixel Format

`capture.rs` delivers **BGRA** buffers. `save_frame_as_png()` already handles the swap. If vision-helper returns garbled coordinates or empty results on a known-good image, verify the channel order hasn't been disturbed.

### Coordinate System

Apple Vision returns **bottom-left origin, normalized** coordinates. `ocr.rs` converts to top-left CSS points. Do not convert coordinates again outside `ocr.rs`.

### Scale Factor

`capture.rs` hardcodes `scale_factor: 2.0` in `OutputHandler`. On non-Retina displays or external monitors, this will cause misaligned overlay boxes. Real scale factor query is deferred (Step 7).

---

## Testing

### Manual Smoke Test

```bash
# 1. Verify sidecar loads the model
./src-tauri/binaries/llama-server-aarch64-apple-darwin \
  --model ~/Library/Application\ Support/contextura/models/qwen3-0.6b-q4_k_m.gguf \
  --port 8765 --n-gpu-layers 99 --host 127.0.0.1 --jinja &
curl http://127.0.0.1:8765/health  # must return {"status":"ok"}

# 2. Verify a translation
curl -X POST http://127.0.0.1:8765/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{"model":"local","messages":[{"role":"system","content":"You are a Japanese-to-English translator. /no_think"},{"role":"user","content":"1: こんにちは"}],"temperature":0.1,"max_tokens":64}'

# 3. Verify vision-helper (needs a real Japanese screenshot)
./src-tauri/binaries/vision-helper-aarch64-apple-darwin /tmp/test-jp.png

# 4. Run the app
cargo tauri dev
```

### Unit Tests

```bash
cd src-tauri && cargo test
```

Existing tests:

- `motion::tests::debounce_should_trigger_when_motion_stops`
- `styling::tests::styling_black_bg_should_return_white_text`
- `styling::tests::styling_white_bg_should_return_black_text`

### E2E Test Suite

`test-corpus/` PNGs are placeholder 0-byte files. `--test-suite` prints mock results. Not usable until the corpus is curated with real Japanese screenshots and the CLI calls the real pipeline.

---

## What NOT to Do

1. **Do not refactor working subsystem modules.** `capture.rs`, `ocr.rs`, `translation.rs`, `styling.rs`, `motion.rs` are all correct. Any remaining problems are orchestration issues in `lib.rs`.

2. **Do not mark TODO items `[x]` unless you've verified end-to-end.** Previous false-done claims are what caused the audit discrepancies that required this file to exist.

3. **Do not add new dependencies** without justification. The tree is already large.

4. **Do not restructure the module layout.** Single-file-per-subsystem is intentional.

5. **Do not implement multi-display, model switching, or wizard screens 2–4** until `Cmd+Shift+T`, `Cmd+Shift+R`, and the watchdog from Step 6–7 work.

6. **Do not add `#[allow(dead_code)]`** to new code. Use `#[expect(dead_code, reason = "...")]` if a lint must be suppressed temporarily.

7. **Do not use NLLB or any encoder-decoder model.** They are fundamentally incompatible with llama-server.

---

## File Reference

| File              | Purpose                               | Read when...                                 |
| ----------------- | ------------------------------------- | -------------------------------------------- |
| `TODO.md`         | Ordered task list, pre-flight checks  | Starting any implementation work             |
| `SPEC.md`         | Full technical specification (v1.5.0) | Needing algorithm details or API contracts   |
| `ARCHITECTURE.md` | Current data flow + module status     | Getting oriented, understanding the pipeline |
| `PRODUCTION.md`   | Audit-accurate production readiness   | Checking what's done vs claimed              |
| `AGENT.md`        | This file — agent instructions        | First thing to read                          |
