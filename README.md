# Contextura

Contextura is a privacy-first, offline-only, real-time Japanese-to-English screen translation overlay for macOS. Built on Apple Silicon native APIs and local LLM inference, it dynamically captures screen text, runs on-device OCR, and overlays translated English text directly on top of the original content.

---

## Key Features

- **Zero-Cloud Dependency:** 100% local execution. Translations and OCR are performed completely offline to protect user privacy.
- **Intelligent Motion Gating:** Deduplicates captured frames using xxHash thumbnail hashing and a settling state machine (`200ms` debounce) to bypass OCR/translation runs during active scrolling.
- **Apple Silicon Native OCR:** Leverages the native Swift Vision API for high-confidence OCR candidate selection and robust reading-order text sorting.
- **Pluggable Translation Engine:** Supports local GGUF models. Centered on **TranslateGemma 4B IT Q4_K_M** (default) and Qwen-style models, running via a managed `llama-server` sidecar.
- **Contrast-Aware Styling:** Samples screenshot background colors and applies WCAG-compliant high-contrast styling for optimal readability.
- **App-Switch Awareness:** Automatically clears overlay content and memory context upon active app switches to prevent overlap leaks.
- **Secure Cache Storage:** Snapshots are written securely to private app-specific cache paths instead of public `/tmp` spaces.

---

## Tech Stack

- **Orchestration:** Rust 2024, Tauri v2
- **OS Frame Capture:** macOS ScreenCaptureKit (`PixelFormat::BGRA`)
- **OCR engine:** Swift `vision-helper` (Apple Vision Framework)
- **LLM sidecar:** `llama-server` (llama.cpp)
- **Frontend:** Vanilla HTML5, CSS3, JavaScript (no build step, transparent overlay)

---

## Setup & Installation

### 1. Prerequisites

Ensure you have Xcode, Apple Command Line Tools, and Rust installed:

```bash
xcodebuild -version
xcode-select -p
rustc --version
cargo --version
python3 --version
```

### 2. Build the Application

Clone the repository and run Tauri in development mode:

```bash
cargo tauri dev
```

_Note: The initial compile will build the Tauri bindings, ScreenCaptureKit abstractions, and Rust modules._

### 3. Deploy the Translation Model

Download the default **TranslateGemma** model directly to the application models folder:

```bash
python3 -m pip install huggingface_hub

huggingface-cli download mradermacher/translategemma-4b-it-GGUF \
  translategemma-4b-it.Q4_K_M.gguf \
  --local-dir ~/Library/Application\ Support/contextura/models/
```

_(Alternative decoder-only Qwen GGUF models can also be placed here; Contextura selects translation strategy and sidecar launch flags automatically per model family.)_

### 4. Grant Permissions

On first launch, Contextura will display a 4-step wizard to guide you through:

1. Screen Recording authorization.
2. Model folder location setup.
3. System hotkey configurations.
4. Active status registration.

---

## Hotkeys & Controls

| Shortcut          | Action                                                     | Status |
| :---------------- | :--------------------------------------------------------- | :----- |
| `Cmd + Shift + T` | Toggle overlay visibility                                  | Active |
| `Cmd + Shift + R` | Force immediate OCR / translation pass (bypasses debounce) | Active |
| `Cmd + Shift + M` | Clear context memory and reset visible overlays            | Active |
| `Cmd + Shift + G` | Cycle to next installed local model and restart runtime    | Active |
| `Cmd + Shift + Q` | Quit Contextura                                            | Active |

---

## Testing & Verification

Contextura is verified by both unit/integration tests and a live E2E runner.

### Run Cargo Checks

```bash
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings
```

Current workspace verification status: Rust test suite reports 106 passing tests.

### Run Golden-File Integration Suite

The test corpus contains real screenshot fixtures (`test-corpus/*.png`) and exact text expectations. Verify the complete OCR + Translation pipeline end-to-end:

```bash
cargo run --manifest-path src-tauri/Cargo.toml -- --debug-cli --test-suite test-corpus
```

### Run CLI OCR Pass

Run the pipeline against any specific PNG file:

```bash
cargo run --manifest-path src-tauri/Cargo.toml -- \
  --debug-cli \
  --input ~/Library/Caches/com.contextura.app/contextura-frame-latest.png \
  --pretty
```

---

## Project Structure

```text
contextura/
├── docs/                      # Architectural specifications and ADRs
├── test-corpus/               # Golden screenshots and E2E JSON expectations
├── src/                       # HTML/CSS/JS overlay and setup wizard
│   ├── index.html
│   ├── overlay.js
│   ├── overlay.css
│   └── wizard.html
└── src-tauri/
    ├── Cargo.toml
    └── src/                   # Rust App Orchestrator
        ├── lib.rs             # App entry, Tauri handlers, and coordination
        ├── scheduler.rs       # Async loop, debounce, & concurrent dispatch
        ├── ocr.rs             # OCR subprocess client and post-filters
        ├── translation.rs     # llama-server sidecar strategy implementations
        ├── capture.rs         # ScreenCaptureKit frame stream handler
        ├── motion.rs          # xxHash frame comparison & DebounceStateMachine
        ├── snapshot.rs        # Secure frame encoding and cache directories
        ├── styling.rs         # Contrast-aware WCAG overlay coloring
        └── cli.rs             # Golden integration test-suite runtime
```

---

## Security & Cache Policies

Contextura does not write temporary frames to shared `/tmp` spaces. Captured frames are securely processed in the private, non-world-readable application cache directory:

- **Storage Path:** `~/Library/Caches/com.contextura.app/`
- **Temporary Snapshots:** `contextura-frame-{id}.png` (cleaned up automatically)
- **Latest Debug Snapshot:** `contextura-frame-latest.png`

## Release Hardening Notes

- **Updater signing key:** Updater endpoints are configured, but `plugins.updater.pubkey` is still empty in `src-tauri/tauri.conf.json` and must be set before production release.
- **Content Security Policy:** The current Tauri config sets `app.security.csp` to null; define a restrictive CSP before production release.
