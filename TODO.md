# TODO.md — Real-Time Screen Translation Overlay

**Development Roadmap & Task Tracker**
**Stack:** Rust · Tauri v2 · Swift subprocesses · llama-server sidecar
**Platform:** macOS 13+ (Apple Silicon)

---

## Status Legend

- `[x]` — Genuinely implemented and working
- `[-]` — Scaffolded / mocked (structure exists, real implementation needed)
- `[ ]` — Not yet started

---

## How to Use This File

- Phases 0, 4, 5, 6 are done. The current focus is **Phase P — Pipeline Activation**.
- After Phase P is complete, Phase 7 (E2E testing) becomes meaningful.
- Phase 8 (distribution) is last.
- Record all architectural decisions in `DECISIONS.md`.

---

## Phase 0 — Environment & Project Setup ✅ Complete

### 0.1 Prerequisites

- [x] Install Rust stable toolchain
- [x] Install Node.js LTS
- [x] Install Tauri CLI v2
- [x] Enroll in Apple Developer Program
- [x] Install Xcode and Command Line Tools
- [x] Verify Metal available

### 0.2 Project Scaffold

- [x] Create Tauri v2 project (`jp-translate`)
- [x] Configure `tauri.conf.json`: transparent, borderless, alwaysOnTop, skipTaskbar
- [x] Add entitlements (`com.apple.security.screen-capture`)

### 0.3 Initial Dependencies

- [x] `tauri`, `objc2`, `crossbeam-channel`, `rayon`, `serde`, `serde_json`, `uuid`
- [x] `log`, `env_logger`, `reqwest`, `clap`, `sentry`

### 0.4 Baseline

- [x] `cargo tauri dev` compiles and launches transparent window

### 0.5 Debug CLI Mode

- [x] `--debug-cli` flag (currently outputs mock data; will be real after Phase P)
- [x] `--debug-cli --once`
- [x] `--debug-cli --test-suite <dir>` (scaffolded)
- [x] `--list-models`, `--prune-models`

### 0.6 Settings File

- [x] `settings.json` created on first run with all defaults
- [x] Read at startup; applied to constants
- [x] Tray → "Open Settings File" (Finder reveal)

---

## Phase 1 — Screen Capture & Motion Detection 🔨 Scaffolded

**Current state:** `capture.rs` has a `thread::spawn` mock that generates fake frames. `motion.rs` logic is implemented but receives mock input. Nothing real runs end-to-end.

**Note: Phase 1 real implementation is handled in Phase P.1 below.**

---

## Phase 2 — OCR Integration 🔨 Scaffolded

**Current state:** `ocr.rs` has `OcrEngine` struct with mock output. `vision-helper.swift` not yet written or compiled. Architecture decision resolved: use Swift subprocess.

**Note: Phase 2 real implementation is handled in Phase P.2 below.**

---

## Phase 3 — Translation Engine 🔨 Scaffolded

**Current state:** `translation.rs` has `TranslationEngine` with mock uppercase translation. `TranslationMemory`, `AppWindowTracker`, and `InvalidationReason` are scaffolded. `llama-server` not yet bundled. Architecture decision resolved: use `llama-server` sidecar.

### 3.2–3.3 Model Storage & RAM Guard

- [x] `manifest.json` management
- [x] Orphan scan + 30-day stale check
- [x] `sysctl hw.memsize` RAM gate (Quality Mode disabled if < 12GB)

### 3.5–3.6 Context Memory & Invalidation

- [x] `TranslationMemory` struct (`push`, `clear`, `as_context_slice`)
- [x] `AppWindowTracker` + `InvalidationReason` channel scaffolded
- [-] `NSWorkspaceDidActivateApplicationNotification` subscription — structure exists but not wired in pipeline loop

### 3.7–3.9 Quality Mode, Watchdog, Thermal

- [x] `switch_model()` scaffolded
- [x] Watchdog thread structure
- [x] IOKit thermal subscription scaffolded
- [-] None of these are connected to a real sidecar yet

**Note: Phase 3 real implementation is handled in Phase P.3 below.**

---

## Phase 4 — Dynamic Styling ✅ Complete

- [x] `fn sample_border_color()` — outer 2px sampling
- [x] `fn relative_luminance()` + `fn linearize_channel()`
- [x] Contrast threshold: `L > 0.179` → black text, else white
- [x] `overlay_bg` at 85% opacity
- [x] Unit tests: white/black/gray/dark-blue backgrounds all pass

---

## Phase 5 — IPC & Frontend Rendering ✅ Complete

- [x] `TranslationBox` + `TranslationPayload` structs with `#[derive(Serialize)]`
- [x] `fn build_payload()` implemented
- [x] All 4 Tauri events emitted (`translation-update`, `translation-clear`, `translation-started`, `translation-error`)
- [x] `display_id` routing to correct window
- [x] Frontend: transparent overlay, absolutely-positioned divs, vertical text (`writing-mode`)
- [x] Spinner on `"translation-started"`, error banner on `"translation-error"`
- [x] CSS `opacity 0.15s ease-in` transition

---

## Phase 6 — Global Hotkeys & App Polish ✅ Complete

- [x] All 5 global shortcuts registered and functional
- [x] Tray menu: all items (toggle, translate now, model status, clear memory, manage models, settings, help, quit)
- [x] Thermal badge on tray icon
- [x] First-run 4-screen wizard (permission, model selection, download, privacy)
- [x] Wizard completion flag; does not re-appear
- [x] `help.html` bundled and accessible from tray
- [x] `tauri-plugin-updater` configured with GitHub Releases endpoint
- [x] `sentry-rust` initialized only if user opted in

---

## Phase P — Pipeline Activation ⬜ Current Focus

**This is the critical phase.** Everything from Phase 1–3 exists as scaffolding. This phase replaces every mock with a real implementation and wires them together in `main.rs`. Complete sub-phases in order.

---

### P.0 — Resolve Pre-flight Questions (Do First)

Before writing any code, answer these two questions and record them in `DECISIONS.md`:

- [x] **Q1: Do you already have `nllb-200-distilled-600M.Q4_K_M.gguf` locally?**
  - Yes → skip model download during development; point sidecar directly at local path
  - No → implement Phase P.3.1 (model downloader) before attempting P.3.2 (sidecar)

- [x] **Q2: Confirmed architecture decisions:**
  - OCR: Swift `vision-helper` subprocess ✅ (resolved)
  - Translation: `llama-server` sidecar ✅ (resolved)
  - Capture: try `screencapturekit` Rust crate first (1-day time-box); if broken, Swift capture helper
  - Record final decisions in `DECISIONS.md`

---

### P.1 — Real Screen Capture

**Goal:** Replace the `thread::spawn` mock in `capture.rs` with actual SCKit frame delivery.

#### P.1.1 — Try `screencapturekit` Rust Crate (1-day time-box)

- [x] Add `screencapturekit` crate to `Cargo.toml`
- [x] In `capture.rs`, remove the mock thread
- [x] Write `DisplayManager::start()`:
  - [x] Request screen recording permission; block with error UI if denied
  - [x] Enumerate displays via `SCShareableContent::get()`
  - [x] For each display: create `SCStream` with `SCContentFilter` + `SCStreamConfiguration`
    - [x] `BGRA8Unorm` pixel format, 30 FPS, full resolution
    - [x] Exclude overlay window via `excludedWindows`
  - [x] In frame callback: extract `CVPixelBuffer`; send to `crossbeam` channel (drop if full)
- [x] Test: print real frame dimensions and timestamp to console at 30 FPS
- [x] If this works: mark done. If crate is broken/incomplete after 1 day: proceed to P.1.2

#### P.1.2 — Swift Capture Helper (Fallback Only)

- [ ] Write `src-tauri/src/bin/capture-helper.swift`:
  - [ ] Creates `SCStream` in Swift, writes frames to a Unix domain socket or named pipe
  - [ ] Rust reads frame bytes from the socket into its `crossbeam` channel
- [ ] Wire Rust to spawn `capture-helper` subprocess and read from its output stream
- [x] Test: print real frame dimensions at 30 FPS

#### P.1.3 — Wire to Motion Detector

- [x] Remove `#[expect(dead_code)]` from `motion.rs`
- [x] In frame-processing thread: pass real `CVPixelBuffer` (or frame bytes) to `MotionDetector`
- [ ] Test: `--debug-cli` output shows "TRIGGERED" ~300ms after stopping scroll
- [ ] Test: blinking cursor does NOT trigger (connected-components check)
- [ ] Test: each display triggers independently (multi-monitor)

#### P.1.4 — Snapshot to PNG

- [x] On `DebounceEvent::Triggered`: encode current pixel buffer as PNG to a temp file
  - [x] Use `image` crate (`image::save_buffer()`) or write raw BGRA bytes then convert
  - [x] Temp path: `/tmp/jp-translate-frame-{frame_id}.png`
- [x] Pass PNG path + `display_id` to OCR channel

**✅ P.1 Milestone:** `--debug-cli` output shows real screen trigger events with correct timing. PNG files appear in `/tmp` on trigger.

---

### P.2 — Real OCR via `vision-helper`

**Goal:** Build the Swift helper, wire it into `OcrEngine`, confirm Japanese text extraction works.

#### P.2.1 — Build `vision-helper` Swift Tool

- [x] Create `src-tauri/src/bin/vision-helper.swift`:

```swift
import Foundation
import Vision
import AppKit

let imagePath = CommandLine.arguments[1]
guard let image = NSImage(contentsOfFile: imagePath),
      let cgImage = image.cgImage(forProposedRect: nil, context: nil, hints: nil) else {
    print("[]"); exit(0)
}

let request = VNRecognizeTextRequest()
request.recognitionLevel = .accurate
request.recognitionLanguages = ["ja-JP"]
request.usesLanguageCorrection = true

let handler = VNImageRequestHandler(cgImage: cgImage)
try? handler.perform([request])

struct Result: Codable {
    let text: String
    let confidence: Float
    let x, y, width, height: Double
    let text_angle: Double
}

var results: [Result] = []
for obs in (request.results ?? []) {
    guard let candidate = obs.topCandidates(1).first else { continue }
    let box = obs.boundingBox
    results.append(Result(
        text: candidate.string,
        confidence: candidate.confidence,
        x: box.origin.x, y: box.origin.y,
        width: box.size.width, height: box.size.height,
        text_angle: Double(obs.yaw?.doubleValue ?? 0.0)
    ))
}

let data = try! JSONEncoder().encode(results)
print(String(data: data, encoding: .utf8)!)
```

- [x] Compile: `swiftc vision-helper.swift -o vision-helper`
- [x] Test manually: `./vision-helper /path/to/japanese.png` outputs valid JSON
- [x] Add compile step to Tauri build pipeline (build script or `build.rs`)
- [x] Bundle compiled binary in app: add to `tauri.conf.json` → `bundle.resources`

#### P.2.2 — Wire `OcrEngine` to Subprocess

- [x] In `ocr.rs`, replace mock output:

```rust
pub fn recognize(&self, png_path: &Path) -> Result<Vec<OcrResult>> {
    let output = Command::new(&self.vision_helper_path)
        .arg(png_path)
        .output()?;
    let raw: Vec<VisionHelperResult> = serde_json::from_slice(&output.stdout)?;
    // ... convert to OcrResult, apply coordinate conversion, filtering
}
```

- [x] Implement coordinate conversion: flip Y-axis, divide by `scale_factor`
- [x] Derive `is_vertical = text_angle.abs() > std::f64::consts::PI / 4.0`
- [x] Apply furigana suppression (already scaffolded — confirm it works with real data)
- [x] Apply confidence filter (< 0.4), CJK filter, IoU merge
- [x] Clean up temp PNG file after subprocess exits

#### P.2.3 — Test OCR End-to-End

- [ ] Run `--debug-cli` with a Japanese webpage open; verify real text appears in JSON output
- [x] Test on vertical manga screenshot; verify `is_vertical: true` and swapped dimensions
- [x] Test furigana-heavy screenshot; verify furigana boxes absent from output
- [ ] Verify bounding box coordinates align visually (draw debug outline if needed)

**✅ P.2 Milestone:** `--debug-cli` shows real Japanese text strings + bounding boxes extracted from screen content.

---

### P.3 — Real Translation via `llama-server` Sidecar

**Goal:** Bundle `llama-server`, manage its lifecycle, call it for real translations.

#### P.3.1 — Obtain Model File

- [x] If NLLB model already present locally: note path; use directly in P.3.2
- [ ] If NOT present: ensure onboarding wizard model downloader works correctly
  - [x] Test: wizard Screen 3 downloads to correct path, SHA256 verifies, manifest updated
  - [x] Do not proceed to P.3.2 until a valid `.gguf` file is confirmed present

#### P.3.2 — Bundle `llama-server` Binary

- [x] Download pre-compiled `llama-server` from official llama.cpp GitHub releases
  - [x] Ensure it is the **macOS ARM64 / Apple Silicon** build with Metal support
  - [x] Verify it runs: `./llama-server --version`
- [x] Place at `src-tauri/binaries/llama-server-aarch64-apple-darwin`
  - [x] Tauri expects the binary name to include the target triple
- [x] Add to `tauri.conf.json`:
  ```json
  { "bundle": { "externalBin": ["binaries/llama-server"] } }
  ```
- [x] Test: `cargo tauri build` includes the binary; `cargo tauri dev` can find it

#### P.3.3 — Implement Sidecar Lifecycle in `translation.rs`

- [x] Remove mock uppercase translation
- [x] Write `TranslationClient` struct:
  - [x] `fn start_sidecar(model_path: &Path, port: u16) -> Result<Child>`
    - [x] Spawn via Tauri shell: `app.shell().sidecar("llama-server").args([...]).spawn()`
    - [x] Args: `--model <path>`, `--port <port>`, `--n-gpu-layers 99`, `--ctx-size 1024`, `--host 127.0.0.1`, `--log-disable`
  - [x] `fn wait_for_ready(port: u16) -> Result<()>`
    - [x] Poll `GET http://127.0.0.1:{port}/health` every 500ms
    - [x] Timeout after 15s; return error
  - [x] `fn translate_batch(&self, strings: &[String], context: &[(String,String)]) -> Result<Vec<String>>`
    - [x] Build batched numbered prompt with context header
    - [x] `POST /v1/chat/completions` with system prompt + user prompt
    - [x] Parse response; extract numbered lines; map to indices
    - [x] `""` for missing/malformed lines
    - [x] Sub-batch at 15 strings
- [x] On app quit: kill the sidecar child process

#### P.3.4 — Wire Watchdog to HTTP Health Check

- [x] Replace old thread-panic watchdog with HTTP-polling watchdog:
  - [x] Background thread polls `GET /health` every 5s
  - [x] On 3 consecutive failures: restart sidecar, wait for ready, emit `"translation-error"`
- [x] Test: manually kill `llama-server`; verify app recovers within ~15s

#### P.3.5 — Wire Context Invalidation

- [x] Remove `#[expect(dead_code)]` from `context.rs`
- [x] Subscribe `NSWorkspaceDidActivateApplicationNotification` in `main.rs` setup
- [x] In translation pipeline loop: drain `invalidation_rx` before each cycle
- [x] Test: switch from Safari to Terminal → memory clears, overlay clears
- [x] Test: switch Safari tabs → memory NOT cleared

#### P.3.6 — Wire Gemma 4 Model Switch

- [x] `switch_model()` restarts `llama-server` sidecar with new `--model` path
- [x] Show "Loading model…" spinner during restart + health check wait
- [x] `Cmd+Shift+G` → `switch_model()` (no-op if RAM < 12GB)

**✅ P.3 Milestone:** `--debug-cli` shows real English translations for real Japanese OCR results. Context memory carries across sequential screens.

---

### P.4 — Wire `main.rs` Pipeline Orchestration

**Goal:** Connect all real subsystems in `main.rs` setup. This is the final wiring step.

- [x] Remove all `#[expect(dead_code)]` annotations from all pipeline modules
- [ ] In `tauri::Builder::setup` closure, implement the full orchestration sequence:
  - [x] Start `llama-server` sidecar; call `wait_for_ready()`
  - [ ] Load settings; initialize `TranslationMemory`
  - [x] Subscribe `NSWorkspaceDidActivateApplicationNotification`
  - [ ] Subscribe IOKit thermal notifications
  - [x] For each display:
    - [ ] Create `SCStream` (or launch capture helper)
    - [ ] Create frame `crossbeam` channel
    - [x] Spawn frame-processing thread:
      ```
      loop {
          frame = frame_rx.recv()
          motion_ratio = motion_detector.update(frame)
          match debounce.update(motion_ratio):
              MotionDetected -> emit "translation-clear"
              Triggered ->
                  png_path = save_frame_as_png(frame)
                  ocr_results = ocr_engine.recognize(png_path)
                  delete_temp_png(png_path)
                  drain invalidation_rx; apply clears
                  emit "translation-started"
                  translations = translation_client.translate_batch(
                      ocr_results.texts(), memory.as_context_slice()
                  )
                  styled_boxes = styling::build_boxes(frame, ocr_results, translations) // Rayon
                  payload = build_payload(styled_boxes, display_id, scale_factor, frame_id)
                  emit "translation-update" to correct window
                  memory.push_all(ocr_results, translations)
      }
      ```
  - [ ] Subscribe `CGDisplayRegisterReconfigurationCallback` for hot-plug

**✅ P.4 Milestone:** Full end-to-end pipeline works. Open a Japanese webpage, stop scrolling, translation boxes appear over the text within 2 seconds. `--debug-cli` shows real timing metrics.

---

### P.5 — Smoke Test & Validation

- [ ] Open Japanese course material website → overlay translates correctly
- [ ] Open manga with vertical text → vertical boxes appear correctly
- [x] Open manga with furigana → furigana suppressed
- [ ] Switch from browser to Terminal → overlay clears, memory clears
- [ ] Press `Cmd+Shift+R` → immediate re-translation without scrolling
- [ ] Press `Cmd+Shift+M` → memory clears; overlay stays
- [ ] Let Mac sit on battery under load → thermal degradation kicks in (longer debounce, NLLB forced)
- [ ] Plug in / cool down → normal behavior restored
- [ ] Unplug external monitor → overlay for that monitor closes gracefully
- [ ] Run `--debug-cli --once` on a Japanese screenshot → JSON output contains real translations

**✅ Phase P Complete Milestone:** The application translates real Japanese screen content end-to-end. Every mock has been replaced.

---

## Phase 7 — Performance, E2E Testing & Hardening 🔨 Scaffolded → Meaningful After Phase P

**Note:** These tasks are scaffolded but cannot produce meaningful results until Phase P is complete.

### 7.1 Performance Profiling

- [ ] Profile with Xcode Instruments: Time Profiler + Allocations
- [ ] Measure end-to-end latency: frame capture → overlay render
- [ ] Verify: NLLB < 2s, Gemma 4 < 5s, sidecar startup < 5s/8s
- [ ] Verify: peak RAM within budget (Default < 3GB, Quality < 8GB)

### 7.2 Optimization

- [ ] If capture slow: pre-allocate thumbnail buffers
- [ ] If translation slow: reduce `--ctx-size`, test smaller batch sizes
- [ ] If overlay jank: batch DOM updates via `requestAnimationFrame`

### 7.3 E2E Test Suite

- [x] Curate `test-corpus/`: at least 10 Japanese PNGs (mix of horizontal, vertical, furigana)
- [ ] Write companion `.expected.json` per PNG
- [ ] Run `--debug-cli --test-suite ./test-corpus`; confirm all pass
- [ ] Set up GitHub Actions: run test suite on every commit
- [x] Include at least 2 vertical-text PNGs and 1 furigana-heavy PNG

### 7.4 Edge Case Hardening

- [ ] Resolution change mid-session: `scale_factor` updates; stream reconfigures
- [ ] System sleep/wake: SCKit streams restart
- [ ] OCR empty → overlay clears
- [ ] Mixed Japanese/English: English strings not sent to translator
- [ ] Simulate RAM < 12GB: Quality Mode disabled
- [ ] 0 displays: app stays alive, logs warning

### 7.5 Memory Leak Check

- [ ] Run 30 minutes; verify no unbounded growth in Activity Monitor
- [ ] No DOM node accumulation over many translation cycles

**✅ Phase 7 Milestone:** E2E suite passes in CI. Latency targets met. Memory stable at 30 minutes.

---

## Phase 8 — Build, Sign & Distribution ⬜ Not Started

### 8.1 Code Signing

- [ ] Configure Tauri signing with Apple Developer certificate
- [ ] Set bundle identifier: `com.yourname.jp-translate`
- [ ] All entitlements in `entitlements.plist`
- [x] **Sign `llama-server` and `vision-helper` binaries separately** — both must be codesigned as part of the app bundle or notarization will fail

### 8.2 Notarization

- [ ] Set up `xcrun notarytool` with App Store Connect API key
- [ ] Add notarization to build script
- [ ] Test: install notarized `.dmg` on clean Mac; verify no Gatekeeper warnings
- [ ] Verify `llama-server` subprocess is not blocked by Gatekeeper on first run

### 8.3 Packaging

- [ ] Ship without model (wizard prompts download on first launch)
- [ ] `llama-server` binary ships inside `.app`
- [ ] Create `.dmg` via Tauri bundler
- [ ] Write `README.md`

### 8.4 Final QA Checklist

- [ ] macOS 13 Ventura (minimum target)
- [ ] macOS 14 Sonoma
- [ ] MacBook Pro Retina (2x scale)
- [ ] External 4K monitor (1x scale)
- [ ] Safari, Chrome, Firefox
- [ ] PDF viewer, Terminal, VS Code
- [ ] Vertical Japanese text
- [ ] First-run wizard on fresh machine
- [ ] RAM < 12GB simulation

**✅ Phase 8 Milestone:** `.dmg` installs cleanly, passes Gatekeeper, full pipeline works on fresh machine.

---

## Backlog / v1.1+

- [ ] Apple Foundation Models as Native Tier (macOS 26+, zero model download)
- [ ] Full settings UI (no manual JSON editing)
- [ ] Tab-level context isolation within same browser
- [ ] Furigana tooltip on hover
- [ ] Persist translation memory across sessions (opt-in)
- [ ] Vocabulary lookup on hover
- [ ] Export translations to clipboard or text file
- [ ] Chinese (Traditional/Simplified) and Korean
- [ ] VoiceOver accessibility
- [ ] Japanese UI localization

---

## Quick Reference: Key File Locations

```
jp-translate/
├── src-tauri/
│   ├── src/
│   │   ├── main.rs              # Pipeline orchestration (Phase P.4)
│   │   ├── capture.rs           # SCKit capture (Phase P.1)
│   │   ├── motion.rs            # Motion detection + debounce (scaffolded)
│   │   ├── ocr.rs               # vision-helper subprocess wrapper (Phase P.2)
│   │   ├── translation.rs       # llama-server HTTP client (Phase P.3)
│   │   ├── context.rs           # AppWindowTracker + InvalidationReason
│   │   ├── thermal.rs           # IOKit thermal monitoring
│   │   ├── styling.rs           # WCAG contrast calculation
│   │   ├── ipc.rs               # Payload types + Tauri event emitter
│   │   ├── downloader.rs        # reqwest model downloader + SHA256
│   │   └── settings.rs          # settings.json read/write
│   ├── src/bin/
│   │   └── vision-helper.swift  # Swift OCR subprocess (Phase P.2.1)
│   ├── binaries/
│   │   └── llama-server-aarch64-apple-darwin  # Pre-compiled sidecar (Phase P.3.2)
│   ├── Cargo.toml
│   ├── tauri.conf.json
│   └── entitlements.plist
├── src/
│   ├── index.html               # Overlay WebView
│   ├── overlay.js               # Event listeners + DOM rendering
│   ├── overlay.css              # Transparent overlay styles
│   ├── wizard.html              # First-run onboarding wizard
│   └── help.html                # Bundled help page
├── test-corpus/                 # PNGs + .expected.json for --test-suite
├── SPEC.md
├── TODO.md
├── PRODUCTION.md
├── DECISIONS.md                 # Architecture decisions log
└── README.md
```
