# TODO.md — Real-Time Screen Translation Overlay

**Development Roadmap & Task Tracker**
**Stack:** Rust · Tauri v2 · ScreenCaptureKit · Apple Vision · llama.cpp
**Platform:** macOS 13+ (Apple Silicon)

---

## How to Use This File

- Work through phases sequentially — each phase builds on the last
- Mark tasks `[x]` as you complete them
- Each phase ends with a **Milestone** you can verify before moving on
- When a key decision is made (e.g., objc2-vision vs Swift subprocess), record it in `DECISIONS.md`

---

## Phase 0 — Environment & Project Setup

_Goal: Have a running Tauri app that opens a transparent window. Add Debug CLI early._

### 0.1 Prerequisites

- [x] Install Rust stable toolchain via `rustup` (`rustup toolchain install stable`)
- [x] Install Node.js LTS (required for Tauri CLI)
- [x] Install Tauri CLI v2: `cargo install tauri-cli --version "^2"`
- [x] Enroll in Apple Developer Program (required for entitlements & notarization)
- [x] Install Xcode and Xcode Command Line Tools
- [x] Verify Metal is available: `system_profiler SPDisplaysDataType | grep Metal`

### 0.2 Project Scaffold

- [x] Create new Tauri v2 project: `cargo tauri init`
- [x] Set project name: `jp-translate`
- [x] Configure `tauri.conf.json`:
  - [x] `"transparent": true`, `"decorations": false`, `"alwaysOnTop": true`, `"skipTaskbar": true`
- [x] Add macOS entitlements file (`entitlements.plist`) with `com.apple.security.screen-capture`
- [x] Link entitlements in `tauri.conf.json` under `bundle.macOS.entitlements`

### 0.3 Initial Dependencies (Cargo.toml)

- [x] `tauri = { version = "2", features = ["..."] }`
- [x] `objc2 = "0.5"`, `objc2-foundation = "0.2"`, `objc2-vision = "0.2"`
- [x] `crossbeam-channel = "0.5"`, `rayon = "1.10"`
- [x] `serde = { version = "1", features = ["derive"] }`, `serde_json = "1"`
- [x] `uuid = { version = "1", features = ["v4"] }`
- [x] `log = "0.4"`, `env_logger = "0.11"`
- [x] `reqwest = { version = "0.12", features = ["stream"] }` (model downloader)
- [x] `clap = { version = "4", features = ["derive"] }` (CLI flags)
- [x] `sentry = "0.34"` (opt-in crash reporting)

### 0.4 Verify Baseline

- [x] `cargo tauri dev` — app compiles and launches
- [x] Confirm window is transparent and borderless
- [x] Confirm window sits on top of other windows

### 0.5 Debug CLI Mode (Add Early — Save Yourself Pain)

- [x] Implement `--debug-cli` flag using `clap`:
  - [x] Skips Tauri window creation entirely
  - [x] Runs full Rust engine (capture → motion → OCR → translation → styling)
  - [x] Prints each trigger's output as pretty-printed JSON to stdout
  - [x] Includes timing fields: `trigger_latency_ms`, `ocr_duration_ms`, `translation_duration_ms`
- [x] Implement `--debug-cli --once`: trigger one OCR cycle then exit
- [x] Implement `--debug-cli --test-suite <dir>`: E2E test runner (see Phase 7.3)
- [x] Implement `--list-models`: print manifest table and exit
- [x] Implement `--prune-models`: interactive cleanup wizard
- [x] Test: `cargo run -- --debug-cli` runs without panicking (output is empty until Phase 1)

> **Why now:** You cannot open browser DevTools on a transparent click-through window while another app is in focus. Build this in Phase 0 and use it throughout Phases 1–4.

### 0.6 Settings File

- [x] On startup, check for `~/Library/Application Support/jp-translate/settings.json`
- [x] If missing, create with defaults:
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
    "active_model": "nllb-600m-q4"
  }
  ```
- [x] Read settings at startup; apply to all relevant constants
- [x] Add tray menu item "Open Settings File" → reveal in Finder via `NSWorkspace::open()`

**✅ Phase 0 Milestone:** Transparent Tauri window opens. `--debug-cli` runs without panic. `settings.json` is created on first run.

---

## Phase 1 — Screen Capture & Motion Detection

_Goal: Capture frames from all displays, detect scroll-stop per display._

### 1.1 Multi-Display ScreenCaptureKit

- [x] Add `objc2-screen-capture-kit` crate (or write manual `objc2` bindings)
- [x] Write a `DisplayManager` struct that:
  - [x] Enumerates all active displays via `NSScreen.screens()` + `SCShareableContent`
  - [x] For each display, creates one `SCStream` with `SCContentFilter` targeting that display
  - [x] Configures `SCStreamConfiguration`: `BGRA8Unorm`, 30 FPS, full display resolution
  - [x] Excludes the corresponding overlay window via `excludedWindows`
  - [x] Delivers frames via a per-display bounded channel (capacity: 2)
- [x] Subscribe to `CGDisplayRegisterReconfigurationCallback`:
  - [x] On display removed: stop stream, drop channel, close Tauri window
  - [x] On display added: create new stream and overlay window
- [x] Request screen recording permission on startup; show error UI if denied
- [x] Test: Print frame dimensions and display ID to console at 30 FPS per display

### 1.2 Frame Pipeline Infrastructure

- [x] Per display: `crossbeam_channel::bounded(2)` → `(frame_tx, frame_rx)`
- [x] In SCStream callback: send to channel; drop if full (backpressure, non-blocking)
- [x] Spawn one dedicated OS thread per display to receive from `frame_rx`
- [x] Test: Confirm frame drops are logged and don't block the capture callback

### 1.3 Motion Detection

- [x] Write `MotionDetector` struct with:
  - [x] `fn downscale_to_thumbnail(buffer: &PixelBuffer) -> GrayImage` (160×90)
  - [x] Edge inset: crop `edge_inset_percent` margin from all sides before comparison
  - [x] `fn compute_diff_mask(prev: &GrayImage, curr: &GrayImage) -> BinaryMask`
    - [x] Per-pixel absolute diff; mark as changed if diff > `pixel_diff_threshold`
  - [x] `fn largest_contiguous_region(mask: &BinaryMask) -> f32`
    - [x] Union-find connected-components pass on mask
    - [x] Return area of the largest single connected region as fraction of total pixels
    - [x] This is `motion_ratio` — not the raw sum of all changed pixels
  - [x] Store previous thumbnail per display
- [x] Write `DebounceStateMachine` enum: `Scrolling | Settling(Instant) | Idle`
  - [x] `fn update(&mut self, motion_ratio: f32) -> DebounceEvent`
  - [x] Returns `DebounceEvent::Triggered` when timer hits 0 in Settling
  - [x] Returns `DebounceEvent::MotionDetected` when motion resets timer
- [x] Test: Blinking cursor and spinning loader do NOT reset the debounce timer

### 1.4 Static Frame Snapshot

- [x] On `DebounceEvent::Triggered`: clone current `PixelBuffer` as snapshot
- [x] Pass snapshot + `display_id` to OCR pipeline via separate channel

**✅ Phase 1 Milestone:** "TRIGGERED" appears ~300ms after stopping scroll on each display. Each display triggers independently.

---

## Phase 2 — OCR Integration (Apple Vision)

_Goal: Extract Japanese text, bounding boxes, orientation, and suppress furigana._

### 2.1 Vision Framework Bindings

- [x] Confirm `objc2-vision` v0.2+ provides `VNRecognizeTextRequest`, `VNRecognizedTextObservation`, `boundingBox`, and `textAngle`
- [x] **Time-box research to 2 hours.** If incomplete or segfaulting, implement Swift CLI fallback:
  - [x] `vision-helper`: ~50-line Swift CLI tool accepting an image path argument
  - [x] Runs `VNRecognizeTextRequest` with `recognitionLanguages: ["ja-JP"]`
  - [x] Prints JSON array: `{ text, confidence, x, y, width, height, text_angle }` to stdout
  - [x] Called from Rust via `std::process::Command`; parse stdout as JSON
  - [x] Fully production-viable — ~5ms process spawn overhead
- [x] Document decision in `DECISIONS.md`

### 2.2 OCR Request Handler

- [x] Write `OcrEngine` struct:
  - [x] `fn recognize(pixel_buffer: &PixelBuffer) -> Vec<OcrResult>`
  - [x] `VNRecognizeTextRequest`: `recognitionLevel = .accurate`, `recognitionLanguages = ["ja-JP"]`, `usesLanguageCorrection = true`
  - [x] Extract `textAngle` per observation; set `is_vertical = |text_angle| > π/4`
  - [x] Return `Vec<OcrResult> { text, confidence, bounding_box, text_angle, is_vertical }`

### 2.3 Coordinate Conversion

- [x] Write `fn vision_to_screen(bbox: NormalizedRect, screen: Size, scale: f32, is_vertical: bool) -> ScreenRect`
  - [x] Flip Y-axis: `screen_y = (1.0 - vision_y - vision_height) * screen_height`
  - [x] Divide by `scale_factor` to get logical CSS points
  - [x] If `is_vertical`: swap width and height in result
- [x] Query `NSScreen.backingScaleFactor` per display; store per `display_id`
- [x] Update on `NSApplicationDidChangeScreenParametersNotification`

### 2.4 Furigana Suppression

- [x] Post-process OCR results after coordinate conversion:
  - [x] For each box, find all others with > 70% horizontal overlap
  - [x] If box height < 40% of the overlapping box → classify as furigana
  - [x] Exclude from translation pipeline; store in parent box's `furigana` field
- [x] Toggle via `settings.json: furigana_suppression`
- [x] Test: Manga screenshot → furigana boxes absent from pipeline output

### 2.5 Result Filtering

- [x] Drop results with `confidence < 0.4`
- [x] Drop results with no CJK characters (`\u{3040}–\u{9FFF}`)
- [x] Merge overlapping bounding boxes (IoU > 0.3)

### 2.6 Testing OCR

- [x] Unit test: load static PNG, run OCR, assert recognized strings
- [x] Test on browser page: verify coordinates in `--debug-cli` output
- [x] Test on vertical manga screenshot: verify `is_vertical = true` and swapped dimensions
- [x] Test furigana suppression: small reading-aid characters excluded

**✅ Phase 2 Milestone:** OCR returns correct strings + orientation from horizontal and vertical Japanese text. Furigana excluded.

---

## Phase 3 — Translation Engine

_Goal: Translate with context memory, crash recovery, thermal awareness, and model management._

### 3.1 Model Downloader & First-Run Wizard

- [x] Build 4-screen onboarding wizard in frontend (HTML/CSS/JS) — `src/wizard.html`:
  - [x] **Screen 1:** Screen recording permission — explain why, open System Settings, poll until granted
  - [x] **Screen 2:** Model selection — Standard (NLLB) vs Quality (Gemma 4); grey out Quality if RAM < 12GB
  - [x] **Screen 3:** In-app download:
    - [x] Rust `download_model` command via `reqwest` chunked streaming
    - [x] Progress bar showing percentage + MB/s
    - [x] Write to `.part` file; rename to `.gguf` only on successful SHA256 verification
    - [x] If wizard closed mid-download: continue in background, show progress in tray
    - [x] On SHA256 mismatch: delete file, show retry dialog
    - [x] Post-download: show 30s interactive demo on bundled sample Japanese image
  - [x] **Screen 4:** Privacy disclosure + Sentry opt-in checkbox + GitHub privacy policy link
- [x] Save wizard completion flag to `settings.json` so it does not re-appear

### 3.2 Model Storage & Manifest

- [x] Create `~/Library/Application Support/jp-translate/models/`
- [x] Create and maintain `models/manifest.json` (id, filename, size_bytes, sha256, downloaded_at, last_used_at, active)
- [x] `fn update_last_used(model_id: &str)` — called on every model load
- [x] `fn scan_for_orphans() -> Vec<PathBuf>` — finds `.gguf` files not in manifest
- [x] On startup: run orphan scan, offer deletion; check 30-day stale non-active models
- [x] Hard 4GB ceiling; block downloads and show cleanup prompt if exceeded

### 3.3 RAM Guard

- [x] At startup: `sysctl hw.memsize` to get total RAM
- [x] If total RAM < 12GB: disable Quality Mode entirely
  - [x] Grey out in tray menu: _"Gemma 4 requires at least 12GB of RAM"_
  - [x] `Cmd+Shift+G` is a no-op; log warning

### 3.4 llama.cpp Integration

- [x] Add `llama_cpp` crate (or `llama-cpp-rs`)
- [x] Write `TranslationEngine`:
  - [x] `fn load(model_path: &Path) -> Result<Self>`: `n_gpu_layers = 99`, context size 1024
  - [x] `fn translate_batch(&self, strings: &[String], context: &[(String, String)]) -> Result<Vec<String>>`
    - [x] Build batched numbered prompt with context memory header if non-empty
    - [x] **Single sequential inference pass — never concurrent on Metal**
    - [x] Parse `^(\d+): (.+)$` per response line; `""` for missing/malformed lines
    - [x] Sub-batch at 15 strings if OCR returns more
- [x] Keep model loaded for entire session

> **Critical:** Do NOT use `rayon::par_iter()` for inference. Metal KV cache allocation is not thread-safe. Concurrent inference calls on the same model will crash.

### 3.5 Rolling Translation Memory

- [x] Add `TranslationMemory` struct:
  ```rust
  struct TranslationMemory {
      entries: VecDeque<(String, String)>,
      max_size: usize,  // from settings.json: context_memory_size (default 6)
  }
  ```
- [x] Implement `fn push()`, `fn clear()`, `fn as_context_slice()`
- [x] After each successful batch: push all `(original, translated)` pairs

### 3.6 Context Invalidation

- [x] Add `InvalidationReason` enum: `AppSwitch { from, to }`, `ManualReset`, `ModelSwitch`
- [x] Create `crossbeam` channel: `invalidation_tx / invalidation_rx`
- [x] Write `AppWindowTracker`:
  - [x] Store `current_bundle_id: Option<String>`
  - [x] Subscribe to `NSWorkspaceDidActivateApplicationNotification` via `objc2-app-kit`
  - [x] On notification: if bundle ID changed → send `InvalidationReason::AppSwitch`
  - [x] Fallback: poll every 2s if subscription fails; log warning
- [x] In translation loop, drain `invalidation_rx` before each cycle:
  - [x] `AppSwitch` → `memory.clear()` + emit `"translation-clear"` + log
  - [x] `ManualReset` → `memory.clear()` only (keep overlay visible)
  - [x] `ModelSwitch` → `memory.clear()` + emit `"translation-clear"`
- [x] Wire `Cmd+Shift+M` → send `ManualReset`
- [x] Wire model switch → send `ModelSwitch`
- [x] Test:
  - [x] Safari → Terminal: memory cleared
  - [x] Safari tab switch: memory NOT cleared (same bundle ID)
  - [x] `Cmd+Shift+M`: memory clears, overlay stays visible
  - [x] Model switch: memory and overlay both clear

### 3.7 Gemma 4 E4B Quality Mode

- [x] Download `gemma-4-e4b-it.Q4_K_M.gguf` during onboarding (if selected) or via tray
- [x] `fn switch_model(new_model_id: &str) -> Result<()>`:
  - [x] Drop current model (GPU buffers released)
  - [x] Show "Loading model…" in overlay + tray
  - [x] Load new model; send `ModelSwitch` invalidation
- [x] `Cmd+Shift+G` → toggle NLLB ↔ Gemma 4 (no-op if RAM < 12GB)
- [x] Check free RAM before switching to Gemma 4; warn if < 8GB free

### 3.8 Crash Recovery Watchdog

- [x] Wrap `TranslationEngine` in a supervised thread
- [x] Count consecutive failures (timeout, Metal error, OOM)
- [x] After 3 failures:
  - [x] Restart thread, reload model
  - [x] Emit `"translation-error"` → frontend banner: _"Translation engine restarted."_ (4s auto-dismiss)
  - [x] Clear overlay until next successful translation

### 3.9 Thermal / Battery Awareness

- [x] Subscribe to IOKit power source notifications
- [x] Subscribe to IOKit thermal notifications
- [x] On battery + thermal state `serious` or `critical`:
  - [x] Force switch to NLLB; send `ModelSwitch` invalidation
  - [x] Set runtime `debounce_ms = 600`
  - [x] Show thermal badge on tray icon
- [x] Restore normal behavior when plugged in or thermal state improves

### 3.10 Parallel Styling (CPU Tasks Only)

- [x] Use `rayon::par_iter()` for: background color sampling, luminance calculation, payload serialization
- [x] Never for inference

### 3.11 Translation Quality Validation

- [x] Build test set of 20 Japanese sentences with known translations
- [x] Include 5 sentences requiring context (dropped subjects, pronouns)
- [x] Run on both NLLB and Gemma 4; document quality findings in `DECISIONS.md`

**✅ Phase 3 Milestone:** Translation works with rolling context. Auto-recovers from crashes. Adapts to thermal state. RAM guard works on sub-12GB systems.

---

## Phase 4 — Dynamic Styling

_Goal: Calculate readable text/background colors for every translation box._

### 4.1 Background Color Sampling

- [x] `fn sample_border_color(buffer: &PixelBuffer, rect: ScreenRect) -> Rgba`
  - [x] Sample outer 2px ring; average RGBA; clamp rect to screen bounds

### 4.2 Contrast Calculation (WCAG 2.1)

- [x] `fn relative_luminance(r: f32, g: f32, b: f32) -> f32`
- [x] `fn linearize_channel(c: f32) -> f32`
- [x] `L > 0.179` → `fg_color = "#000000"`, else `fg_color = "#FFFFFF"`
- [x] `overlay_bg` = sampled color at 85% opacity

### 4.3 Unit Tests

- [x] White background → black text
- [x] Black background → white text
- [x] Mid-gray (#808080) → correct threshold behavior
- [x] Dark blue → white text

**✅ Phase 4 Milestone:** All unit tests pass.

---

## Phase 5 — IPC & Frontend Rendering

_Goal: Render boxes on correct display with vertical text support and status feedback._

### 5.1 IPC Payload Assembly

- [x] `TranslationBox` struct with `#[derive(Serialize)]`: include `is_vertical: bool`, `display_id: u32`
- [x] `fn build_payload(ocr, translations, display_id, scale_factor, frame_id) -> TranslationPayload`

### 5.2 Tauri Event Emission

- [x] Emit `"translation-started"` when inference begins (triggers spinner)
- [x] Emit `"translation-update"` with full payload when complete
- [x] Emit `"translation-clear"` when motion detected or app switched
- [x] Emit `"translation-error"` when watchdog restarts engine
- [x] Route each event to the correct window using `display_id`

### 5.3 Frontend HTML/CSS/JS

- [x] `<body>`: `margin: 0; overflow: hidden; background: transparent`
- [x] `#overlay`: `position: fixed; inset: 0; pointer-events: none`
- [x] `"translation-update"` → clear existing boxes, render new divs with dynamic styles
- [x] For `is_vertical` boxes: add `writing-mode: vertical-rl; text-orientation: mixed`
- [x] `"translation-clear"` → remove all boxes
- [x] `"translation-started"` → show bottom-right spinner _"Translating…"_ (opacity 0.6)
- [x] `"translation-error"` → show top banner, auto-dismiss after 4s
- [x] CSS transition: `opacity 0.15s ease-in` on box appearance

### 5.4 Visual Alignment Testing

- [x] Horizontal text on Japanese webpage — Retina + external monitor
- [x] Vertical text on manga screenshot
- [x] Box positions at 1x and 2x `scale_factor`
- [x] Spinner and error banner appear/dismiss correctly

**✅ Phase 5 Milestone:** Overlays appear on all displays. Vertical text renders correctly. Spinner and error banner work.

---

## Phase 6 — Global Hotkeys & App Polish

_Goal: Full UX control, tray menu, first-run wizard, help, auto-update, crash reporting._

### 6.1 Global Shortcuts

- [x] Add `tauri-plugin-global-shortcut`
- [x] Register all 5 hotkeys (functional even when overlay is not focused):
  - [x] `Cmd+Shift+T` → toggle overlay visibility
  - [x] `Cmd+Shift+Q` → quit application
  - [x] `Cmd+Shift+R` → force OCR, bypass debounce
  - [x] `Cmd+Shift+M` → send `ManualReset` invalidation
  - [x] `Cmd+Shift+G` → trigger model switch (no-op if RAM < 12GB)
- [x] Test: all hotkeys work when browser is frontmost

### 6.2 System Tray / Menu Bar Icon

- [x] Menu items:
  - [x] Enable / Disable Overlay (toggle)
  - [x] Translate Now (force re-trigger)
  - [x] Active Model status + toggle
  - [x] Clear Context Memory
  - [x] Manage Models (cleanup UI)
  - [x] Open Settings File (Finder reveal)
  - [x] Help (bundled HTML page)
  - [x] Quit
- [x] Thermal badge on tray icon when degraded mode is active
- [x] Download progress shown in tray if background download is running

### 6.3 First-Run Wizard

- [x] Wizard flows through 4 screens as described in Phase 3.1
- [x] Wizard completion flag saved to settings; does not re-appear on relaunch

### 6.4 In-App Help

- [x] Create `src/help.html` bundled with the app
- [x] Cover: permission setup, all 5 hotkeys, model switching, context memory, FAQ
- [x] Tray → "Help" opens the bundled page

### 6.5 Auto-Update

- [x] Add `tauri-plugin-updater`
- [x] Configure GitHub Releases endpoint in `tauri.conf.json`
- [x] Silent check on startup; tray notification if update available
- [x] User always opts in — never forced

### 6.6 Crash Reporting

- [x] Initialize `sentry-rust` only if user opted in during wizard Screen 4
- [x] Transmit only anonymous stack trace + app version
- [x] Preference changeable via tray → Settings → Privacy

**✅ Phase 6 Milestone:** All hotkeys work. Tray complete. Wizard runs on first launch. Help opens. Auto-update checks silently.

---

## Phase 7 — Performance, E2E Testing & Hardening

_Goal: Hit latency/memory targets, E2E test suite, full edge case coverage._

### 7.1 Performance Profiling

- [x] Profile with Xcode Instruments: Time Profiler + Allocations
- [x] Measure end-to-end latency: frame capture → overlay render
- [x] Measure peak RAM in Default and Quality mode
- [x] Verify targets: NLLB < 2s, Gemma 4 < 5s, model load < 5s/8s

### 7.2 Optimization Tasks

- [x] If motion detection slow: pre-allocate thumbnail buffers, avoid heap alloc in hot path
- [x] If translation slow: reduce `n_ctx`, experiment with batch sizes
- [x] If overlay jank: batch DOM updates via `requestAnimationFrame`

### 7.3 E2E Test Suite (`--test-suite`)

- [x] Curate test corpus in `test-corpus/`: at least 10 Japanese PNG screenshots
- [x] Write companion `.expected.json` per PNG:
  ```json
  { "ocr_must_contain": ["日本語"], "translation_must_contain": ["Japanese"] }
  ```
- [x] Implement `--debug-cli --test-suite <dir>`:
  - [x] Run full pipeline per PNG; assert OCR substrings + translation similarity
  - [x] Print pass/fail per image; exit `0` (all pass) or `1` (any fail)
- [x] Set up CI (GitHub Actions): run test suite on every commit
- [x] Include at least 2 vertical-text PNGs and 1 furigana-heavy PNG in corpus

### 7.4 Edge Case Hardening

- [x] Resolution change mid-session: update `scale_factor` + stream config per display
- [x] System sleep/wake: restart SCKit streams on wake
- [x] OCR empty results: overlay clears gracefully
- [x] Mixed Japanese/English: English strings not passed to translator
- [x] Simulate RAM < 12GB: Quality Mode disabled at startup
- [x] No displays connected: app stays alive, logs warning

### 7.5 Memory Leak Check

- [x] Run app 30 minutes; monitor in Activity Monitor
- [x] No unbounded growth in frame buffer allocations
- [x] No WebView DOM node accumulation over many translation cycles

**✅ Phase 7 Milestone:** E2E suite passes in CI. Latency < 2s (Default), < 5s (Quality). Memory stable after 30 minutes.

---

## Phase 8 — Build, Sign & Distribution

_Goal: A distributable .app that installs cleanly on a fresh machine._

### 8.1 Code Signing

- [ ] Configure Tauri signing with Apple Developer certificate
- [ ] Set bundle identifier: `com.yourname.jp-translate`
- [ ] All required entitlements in `entitlements.plist`

### 8.2 Notarization

- [ ] Set up `xcrun notarytool` with App Store Connect API key
- [ ] Add notarization step to build script
- [ ] Test: install notarized `.dmg` on clean Mac; verify no Gatekeeper warnings

### 8.3 Packaging

- [ ] Ship without model (wizard prompts download on first launch)
- [ ] Create `.dmg` installer via Tauri bundler
- [ ] Write `README.md`: install instructions, first-run guide, model storage location

### 8.4 Final QA Checklist

- [ ] macOS 13 Ventura (minimum target)
- [ ] macOS 14 Sonoma
- [ ] MacBook Pro Retina (2x scale)
- [ ] External 4K monitor (1x scale)
- [ ] Safari, Chrome, Firefox
- [ ] PDF viewer, Terminal, VS Code
- [ ] Vertical Japanese text (manga screenshot)
- [ ] First-run wizard on a machine with no prior install
- [ ] Simulate RAM < 12GB: Quality Mode disabled

**✅ Phase 8 Milestone:** `.dmg` installs cleanly, passes Gatekeeper, full pipeline works including vertical text and first-run wizard.

---

## Backlog / Future Versions (v1.1+)

- [ ] Full settings UI (no more manual JSON editing)
- [ ] Window-specific capture (translate only one app)
- [ ] Tab-level context isolation within the same browser
- [ ] Furigana tooltip on hover (framework in place from Phase 2.4)
- [ ] Persist translation memory across sessions (opt-in, per-app)
- [ ] Vocabulary lookup on hover (dictionary entry)
- [ ] Export translations to clipboard or text file
- [ ] Support Chinese (Traditional/Simplified) and Korean
- [ ] VoiceOver accessibility support
- [ ] Localization: Japanese UI strings for onboarding

---

## Quick Reference: Key File Locations

```
jp-translate/
├── src-tauri/
│   ├── src/
│   │   ├── main.rs              # Tauri app entry + CLI flag dispatch
│   │   ├── capture.rs           # ScreenCaptureKit multi-display
│   │   ├── motion.rs            # Delta check + connected-components + debounce
│   │   ├── ocr.rs               # Apple Vision OCR + furigana suppression
│   │   ├── translation.rs       # llama.cpp + rolling memory + watchdog
│   │   ├── context.rs           # AppWindowTracker + InvalidationReason channel
│   │   ├── thermal.rs           # IOKit power + thermal monitoring
│   │   ├── styling.rs           # Color sampling + WCAG contrast
│   │   ├── ipc.rs               # Payload types + Tauri event emitter
│   │   ├── downloader.rs        # reqwest model downloader + SHA256 verify
│   │   └── settings.rs          # settings.json read/write
│   ├── Cargo.toml
│   ├── tauri.conf.json
│   └── entitlements.plist
├── src/
│   ├── index.html               # Overlay WebView
│   ├── overlay.js               # Event listeners + DOM rendering
│   ├── overlay.css              # Transparent overlay styles
│   ├── wizard.html              # First-run onboarding wizard
│   └── help.html                # Bundled in-app help page
├── test-corpus/                 # PNGs + .expected.json for --test-suite
├── SPEC.md
├── TODO.md
├── PRODUCTION.md
├── DECISIONS.md                 # Log of key technical decisions made during build
└── README.md
```
