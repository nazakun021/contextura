# Contextura

Real-time Japanese→English screen translation overlay for macOS.

Contextura captures your screen, detects when you stop scrolling, extracts Japanese text using Apple's Vision framework, and renders English translations as a transparent overlay precisely positioned over the original text — entirely offline.

**Platform:** macOS 13+ · Apple Silicon (M1/M2/M3/M4) only  
**Status:** Pipeline complete — requires Qwen3-0.6B model download to function

---

## Features

- **Transparent overlay** — absolutely positioned translation boxes layered over any app
- **Fully offline** — Apple Vision OCR + local Qwen3-0.6B via llama-server, no cloud calls
- **Motion-gated inference** — 160×90 pixel diff detection prevents OCR during scrolling
- **Dynamic styling** — WCAG 2.1 background sampling for readable contrast on any content
- **Rolling context memory** — up to 6 prior translation pairs for consistent terminology
- **Thermal aware** — backs off processing under thermal pressure
- **Context invalidation** — clears translation memory when you switch to a different app
- **System tray** + global hotkeys for seamless control

---

## Prerequisites

- macOS 13 Ventura or later (Apple Silicon)
- [Rust toolchain](https://rustup.rs/) (stable)
- Xcode Command Line Tools (`xcode-select --install`)
- Screen Recording permission (granted via first-run wizard)

---

## Setup

### 1. Clone and build

```bash
git clone <repo-url>
cd contextura
cargo tauri dev
```

The first build takes several minutes as llama.cpp and SCKit bindings compile.

### 2. Download the translation model

Contextura uses **Qwen3-0.6B Q4_K_M** (~350MB). NLLB and other encoder-decoder models are **not compatible** with the bundled llama-server.

```bash
# Install huggingface-cli if needed
pip install huggingface_hub

# Download the model
huggingface-cli download Qwen/Qwen3-0.6B-GGUF \
  qwen3-0.6b-q4_k_m.gguf \
  --local-dir ~/Library/Application\ Support/contextura/models/
```

### 3. Verify setup

```bash
# Confirm the model loaded
./src-tauri/binaries/llama-server-aarch64-apple-darwin \
  --model ~/Library/Application\ Support/contextura/models/qwen3-0.6b-q4_k_m.gguf \
  --port 8765 --n-gpu-layers 99 --host 127.0.0.1 --jinja &

curl http://127.0.0.1:8765/health
# Expected: {"status":"ok"}
```

### 4. Grant Screen Recording permission

On first launch, a setup wizard will guide you through granting Screen Recording permission. Contextura cannot capture any frames without this entitlement.

To skip the wizard (development only):

```bash
# Already set to true if you've run the app once
cat ~/Library/Application\ Support/contextura/settings.json
```

---

## Running

```bash
# Development (hot-reload for frontend changes)
cargo tauri dev

# Headless model listing
cargo run -- --list-models

# Debug CLI mode
cargo run -- --debug-cli
```

---

## ⌨️ Hotkeys

| Shortcut      | Action                    | Status     |
| ------------- | ------------------------- | ---------- |
| `Cmd+Shift+T` | Toggle overlay visibility | ⚠️ Stub    |
| `Cmd+Shift+R` | Force immediate OCR scan  | ⚠️ Stub    |
| `Cmd+Shift+M` | Clear translation memory  | ✅ Working |
| `Cmd+Shift+G` | Switch model tier         | ⚠️ Stub    |
| `Cmd+Shift+Q` | Quit application          | ✅ Working |

---

## Usage

1. **Open any app containing Japanese text** — websites, PDFs, manga readers, games
2. **Scroll or navigate**, then **stop moving** — Contextura detects motion and waits for the screen to settle (~300ms debounce)
3. **Translations appear** as semi-transparent overlay boxes over each detected text region
4. **Switch apps** — the overlay clears automatically when you switch to a different application
5. **Force translate** — press `Cmd+Shift+R` to bypass the debounce and translate immediately (once hotkey is implemented)

---

## Configuration

Settings are stored at `~/Library/Application Support/contextura/settings.json`:

```json
{
  "debounce_ms": 300,
  "motion_threshold": 0.05,
  "pixel_diff_threshold": 15,
  "capture_fps": 30,
  "edge_inset_percent": 5,
  "furigana_suppression": true,
  "show_original_text": false,
  "context_memory_size": 6,
  "active_model": "qwen3-0.6b-q4",
  "wizard_completed": true
}
```

| Field                  | Description                                                   |
| ---------------------- | ------------------------------------------------------------- |
| `debounce_ms`          | Milliseconds of stillness before OCR triggers                 |
| `motion_threshold`     | Motion ratio above which screen is "scrolling" (0.0–1.0)      |
| `pixel_diff_threshold` | Per-pixel diff required to count as "changed" (0–255)         |
| `edge_inset_percent`   | % of edges excluded from motion detection (hides cursor/dock) |
| `furigana_suppression` | Skip small text boxes overlapping larger ones (furigana)      |
| `context_memory_size`  | Rolling translation context entries to send with each batch   |
| `active_model`         | Must match `id` field in `manifest.json`                      |

---

## Project Structure

```
contextura/
├── src/                         # Frontend (no build step)
│   ├── index.html               # Overlay page
│   ├── overlay.js               # IPC event listeners + DOM rendering
│   ├── overlay.css              # Transparent overlay styles
│   ├── wizard.html              # First-run wizard
│   └── help.html                # Help page
├── src-tauri/
│   ├── src/
│   │   ├── lib.rs               # App entry + full pipeline orchestration
│   │   ├── capture.rs           # ScreenCaptureKit frame delivery
│   │   ├── motion.rs            # Motion detection + debounce state machine
│   │   ├── ocr.rs               # vision-helper subprocess + post-processing
│   │   ├── translation.rs       # llama-server HTTP client + memory
│   │   ├── styling.rs           # WCAG 2.1 dynamic contrast
│   │   ├── ipc.rs               # IPC payload structs
│   │   ├── context.rs           # NSWorkspace app-switch tracker
│   │   ├── thermal.rs           # IOKit thermal monitor
│   │   ├── hotkeys.rs           # Global shortcuts
│   │   ├── tray.rs              # System tray menu
│   │   ├── settings.rs          # settings.json
│   │   └── cli.rs               # CLI flag handling
│   ├── binaries/
│   │   ├── llama-server-aarch64-apple-darwin
│   │   ├── vision-helper-aarch64-apple-darwin
│   │   └── lib*.dylib (llama.cpp runtime)
│   ├── capabilities/default.json
│   └── tauri.conf.json
├── test-corpus/                 # Japanese PNG test images
├── AGENT.md                     # AI agent instructions (read first)
├── SPEC.md                      # Technical specification
├── TODO.md                      # Phase P.Complete task tracker
├── ARCHITECTURE.md              # This architecture overview
└── PRODUCTION.md                # Production readiness checklist
```

---

## Model Compatibility

Only **decoder-only** transformer models work with the bundled `llama-server`. Encoder-decoder models (NLLB, T5, BART, MarianMT) are **not supported** — llama.cpp will immediately exit with `unknown model architecture`.

**Supported model families:** LLaMA, Qwen, Mistral, Gemma, Phi, DeepSeek, and other decoder-only architectures in GGUF format.

**Planned tiers:**

| Tier                   | Model                   | Size    | RAM              |
| ---------------------- | ----------------------- | ------- | ---------------- |
| **Standard** (default) | Qwen3-0.6B Q4_K_M       | ~350 MB | Low              |
| **Quality** (deferred) | Gemma 4 E4B IT Q4_K_M   | ~5 GB   | Higher           |
| **Native (v1.1)**      | Apple Foundation Models | 0 MB    | None (macOS 26+) |

---

## Privacy

- All processing is local — screen contents never leave your device
- `llama-server` binds only to `127.0.0.1:8765`
- Optional Sentry crash reporting (opt-in via first-run wizard — not yet implemented)
- Network: model download only (Hugging Face, one-time)

---

## License

MIT
