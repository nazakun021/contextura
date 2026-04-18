# TODO.md — Real-Time Screen Translation Overlay

**Development Roadmap & Task Tracker**  
**Stack:** Rust · Tauri v2 · ScreenCaptureKit · Apple Vision · llama.cpp  
**Platform:** macOS 13+ (Apple Silicon)

---

## How to Use This File

- Work through phases sequentially — each phase builds on the last
- Mark tasks `[x]` as you complete them
- Each phase ends with a **Milestone** you can verify before moving on
- Estimated times are rough; adjust based on your Rust familiarity

---

## Phase 0 — Environment & Project Setup

_Goal: Have a running Tauri app that opens a transparent window._

### 0.1 Prerequisites

- [ ] Install Rust stable toolchain via `rustup` (`rustup toolchain install stable`)
- [ ] Install Node.js LTS (required for Tauri CLI)
- [ ] Install Tauri CLI v2: `cargo install tauri-cli --version "^2"`
- [ ] Enroll in Apple Developer Program (required for entitlements & notarization)
- [ ] Install Xcode and Xcode Command Line Tools (required for macOS SDK headers)
- [ ] Verify Metal is available: `system_profiler SPDisplaysDataType | grep Metal`

### 0.2 Project Scaffold

- [ ] Create new Tauri v2 project: `cargo tauri init`
- [ ] Set project name: `jp-translate`
- [ ] Configure `tauri.conf.json`:
  - [ ] Set window to fullscreen display size
  - [ ] Set `"transparent": true`
  - [ ] Set `"decorations": false`
  - [ ] Set `"alwaysOnTop": true`
  - [ ] Set `"skipTaskbar": true`
- [ ] Add macOS entitlements file (`entitlements.plist`) with `com.apple.security.screen-capture`
- [ ] Link entitlements file in `tauri.conf.json` under `bundle.macOS.entitlements`

### 0.3 Initial Dependencies (Cargo.toml)

- [ ] Add `tauri = { version = "2", features = ["..."] }`
- [ ] Add `objc2 = "0.5"`
- [ ] Add `objc2-foundation = "0.2"`
- [ ] Add `crossbeam-channel = "0.5"`
- [ ] Add `rayon = "1.10"`
- [ ] Add `serde = { version = "1", features = ["derive"] }`
- [ ] Add `serde_json = "1"`
- [ ] Add `uuid = { version = "1", features = ["v4"] }`
- [ ] Add `log = "0.4"` and `env_logger = "0.11"`

### 0.4 Verify Baseline

- [ ] Run `cargo tauri dev` — app compiles and launches
- [ ] Confirm window is transparent and borderless
- [ ] Confirm window sits on top of other windows

### 0.5 Debug CLI Mode (Add Early — Save Yourself Pain)

- [ ] Add a top-level CLI argument parser using `clap` crate
- [ ] Implement `--debug-cli` flag that:
  - [ ] Skips Tauri window creation entirely
  - [ ] Runs the full Rust engine (capture → motion → OCR → translation → styling)
  - [ ] Prints each trigger's output as pretty-printed JSON to `stdout`
  - [ ] Includes timing fields: `trigger_latency_ms`, `ocr_duration_ms`, `translation_duration_ms`
- [ ] Implement `--debug-cli --once` sub-flag: trigger exactly one OCR cycle then exit
- [ ] Implement `--list-models` flag: print model manifest and exit
- [ ] Implement `--prune-models` flag: interactive cleanup wizard (no GUI needed)
- [ ] Test: `cargo run -- --debug-cli` captures screen, prints JSON on scroll-stop

> **Why now, not later:** You cannot easily open browser DevTools on a transparent click-through window while another app is in focus. Without this mode, debugging Phase 2–4 means guessing from indirect evidence. Build it in Phase 0 and use it throughout every subsequent phase.

**✅ Phase 0 Milestone:** A fullscreen transparent Tauri window opens on launch with no errors. `cargo run -- --debug-cli` also runs without panicking (even if it prints nothing yet).

---

## Phase 1 — Screen Capture & Motion Detection

_Goal: Capture screen frames and correctly detect when the user stops scrolling._

### 1.1 ScreenCaptureKit Bindings

- [ ] Add `objc2-screen-capture-kit` crate (or write manual `objc2` bindings)
- [ ] Write a `ScreenCapturer` struct that:
  - [ ] Queries available displays via `SCShareableContent`
  - [ ] Creates an `SCStream` targeting the primary display
  - [ ] Configures `SCStreamConfiguration`:
    - [ ] `pixelFormat = kCVPixelFormatType_32BGRA`
    - [ ] `width / height` = display resolution
    - [ ] `minimumFrameInterval = 1/30s`
  - [ ] Excludes the overlay window from the stream via `excludedWindows`
  - [ ] Delivers frames to a Rust closure via `SCStreamOutput` delegate
- [ ] Request screen recording permission on startup; show error UI if denied
- [ ] Test: Print frame dimensions and timestamp to console at 30 FPS

### 1.2 Frame Pipeline Infrastructure

- [ ] Create a bounded `crossbeam_channel::bounded(2)` channel: `frame_tx / frame_rx`
- [ ] In the SCStream callback: send frame to channel (drop if full — backpressure)
- [ ] Spawn a dedicated OS thread to receive from `frame_rx`
- [ ] Test: Confirm frame drops are logged and do not block the capture callback

### 1.3 Motion Detection

- [ ] Write a `MotionDetector` struct with:
  - [ ] `fn downscale_to_thumbnail(buffer: &PixelBuffer) -> GrayImage` (160×90)
  - [ ] Edge inset: crop 5% margin from all sides before comparison
  - [ ] `fn compute_diff_mask(prev: &GrayImage, curr: &GrayImage) -> BinaryMask`
    - [ ] Per-pixel absolute difference; mark as "changed" if diff > `PIXEL_DIFF_THRESHOLD` (default: 15)
  - [ ] `fn largest_contiguous_region(mask: &BinaryMask) -> f32`
    - [ ] Run a union-find (or simple flood fill) connected-components pass on the mask
    - [ ] Return the area of the **largest single connected region** as a fraction of total pixels
    - [ ] This is your `motion_ratio` — replaces the raw sum-of-diffs approach
  - [ ] Store previous thumbnail for comparison
- [ ] Write a `DebounceStateMachine` enum: `Scrolling | Settling(Instant) | Idle`
  - [ ] `fn update(&mut self, motion_ratio: f32) -> DebounceEvent`
  - [ ] Returns `DebounceEvent::Triggered` when timer hits 0 in Settling state
  - [ ] Returns `DebounceEvent::MotionDetected` when motion resets the timer
- [ ] **Why connected-components:** Raw pixel sum counts blinking cursors, spinning loaders, and looping ads as "motion" — these produce scattered isolated pixels that sum to a large ratio but never form a large contiguous block. Scrolling always produces one large cohesive moving region. This single change makes the debounce dramatically more reliable.
- [ ] Test: Console shows correct transitions while scrolling. A blinking cursor or spinner does NOT reset the timer.

### 1.4 Static Frame Snapshot

- [ ] When `DebounceEvent::Triggered` fires, clone the current `PixelBuffer` as a snapshot
- [ ] Pass snapshot to the OCR pipeline via a separate `crossbeam` channel

**✅ Phase 1 Milestone:** Console logs show correct state machine transitions. "TRIGGERED" appears ~300ms after you stop scrolling a webpage.

---

## Phase 2 — OCR Integration (Apple Vision)

_Goal: Extract Japanese text and bounding boxes from a static frame._

### 2.1 Vision Framework Bindings

- [ ] Research `objc2-vision` crate — determine if `VNRecognizeTextRequest` is available and complete
- [ ] **Time-box this research to 2 hours.** If the structs you need are missing or segfault, do NOT spend days writing unsafe FFI by hand.
- [ ] **Primary path:** Use `objc2-vision` if `VNRecognizeTextRequest`, `VNRecognizedTextObservation`, and `boundingBox` are all accessible
- [ ] **Fallback path (implement in parallel, takes ~1 hour):** Write a Swift CLI helper:
  - [ ] `vision-helper` — a ~50-line Swift command-line tool
  - [ ] Accepts an image file path as argument
  - [ ] Runs `VNRecognizeTextRequest` with `recognitionLanguages: ["ja-JP"]`
  - [ ] Prints a JSON array of `{ text, confidence, x, y, width, height }` to stdout
  - [ ] Call from Rust via `std::process::Command::new("vision-helper").arg(image_path)`
  - [ ] Parse stdout as JSON in Rust
  - [ ] **This bridge is fully production-viable** — Vision runs in a separate process, latency cost is negligible (~5ms process spawn), and you're not blocked by crate maturity
- [ ] Decide which path to use based on your 2-hour research; document the decision in a `DECISIONS.md`

### 2.2 OCR Request Handler

- [ ] Write an `OcrEngine` struct with:
  - [ ] `fn recognize(pixel_buffer: &PixelBuffer) -> Vec<OcrResult>`
  - [ ] Configure `VNRecognizeTextRequest`:
    - [ ] `recognitionLevel = .accurate`
    - [ ] `recognitionLanguages = ["ja-JP"]`
    - [ ] `usesLanguageCorrection = true`
  - [ ] Execute request via `VNImageRequestHandler` with `CVPixelBuffer` input
  - [ ] Parse `VNRecognizedTextObservation` array into `Vec<OcrResult>`

### 2.3 Coordinate Conversion

- [ ] Write `fn vision_to_screen(bbox: NormalizedRect, screen: Size, scale: f32) -> ScreenRect`
  - [ ] Flip Y-axis: `screen_y = (1.0 - vision_y - vision_height) * screen_height`
  - [ ] Convert from physical pixels to logical points: `divide by scale_factor`
- [ ] Query `NSScreen.mainScreen.backingScaleFactor` at startup to get `scale_factor`
- [ ] Store `scale_factor` as app-level state; update on `NSApplicationDidChangeScreenParametersNotification`

### 2.4 Result Filtering

- [ ] Drop results where `confidence < 0.4`
- [ ] Drop results where text contains no CJK characters (check Unicode ranges `\u{3040}–\u{9FFF}`)
- [ ] Implement bounding box merge for overlapping results (IoU > 0.3)

### 2.5 Testing OCR

- [ ] Create a test: load a static PNG of Japanese text, run OCR, assert recognized strings
- [ ] Test on browser page with Japanese text — print results to console
- [ ] Verify bounding boxes are visually correct (draw debug outlines in a test mode)

**✅ Phase 2 Milestone:** Running OCR on a Japanese webpage returns correct text strings with accurate screen coordinates printed to console.

---

## Phase 3 — Translation Engine

_Goal: Translate extracted Japanese strings to English using a local model._

### 3.1 Model Download & Storage

- [ ] Create model storage directory: `~/Library/Application Support/jp-translate/models/`
- [ ] Download `nllb-200-distilled-600M.Q4_K_M.gguf` from Hugging Face
- [ ] Verify model file size is ~1.2GB
- [ ] Write a first-launch check: if model missing, show download prompt UI
- [ ] Create `models/manifest.json` to track downloaded models:
  - [ ] Fields per entry: `id`, `filename`, `size_bytes`, `sha256`, `downloaded_at`, `last_used_at`, `active`
- [ ] Write `fn update_last_used(model_id: &str)` — called every time the model is loaded
- [ ] Write `fn scan_for_orphans() -> Vec<PathBuf>` — finds `.gguf` files not in manifest
- [ ] On startup: run orphan scan; if orphans found, offer to delete them (never silently)
- [ ] On startup: if any non-active model's `last_used_at` > 30 days ago, prompt user to prune
- [ ] Enforce a 4GB hard warning ceiling for total models directory size; block new downloads and show cleanup prompt if exceeded
- [ ] Implement `--prune-models` CLI wizard (list models with sizes, confirm deletion interactively)

### 3.2 llama.cpp Integration

- [ ] Add `llama_cpp` crate to `Cargo.toml` (or use `llama-cpp-rs`)
- [ ] Write a `TranslationEngine` struct with:
  - [ ] `fn load(model_path: &Path, model_id: &str) -> Result<Self>`
    - [ ] Load model with `n_gpu_layers = 99` (full Metal offload)
    - [ ] Set context size to `1024` (larger than 512 — needed to fit context memory + batch prompt)
  - [ ] `fn translate_batch(&self, strings: &[String], context: &[(String, String)]) -> Result<Vec<String>>`
    - [ ] If `context` is non-empty, prepend context block to prompt:

      ```
      Previous context (do not retranslate, use for reference only):
      - {context[0].0} → "{context[0].1}"
      ...up to 6 entries

      Translate each numbered Japanese string to English.
      Output only the translations, one per line, same numbered format.
      1: {strings[0]}
      2: {strings[1]}
      ```

    - [ ] Run **one sequential inference pass** — do NOT submit concurrent inference calls
    - [ ] Parse response: split by newlines, match `^(\d+): (.+)$` per line
    - [ ] Map results back to original indices; use `""` for missing/malformed lines
    - [ ] If `strings.len() > 15`, split into sequential sub-batches of 15

- [ ] Keep model loaded for the entire app session (no reload per request)

> **Critical — do NOT use `rayon::par_iter()` for inference:** Metal's memory model does not safely support concurrent inference contexts on the same model instance. Two threads simultaneously allocating KV cache on the same GPU backend causes memory contention and potential Metal driver crashes. Rayon is still appropriate for CPU-bound work (color sampling, coordinate math, serialization) — just never for `llama.cpp` inference calls.

### 3.3 Rolling Translation Memory

- [ ] Add a `TranslationMemory` struct to app state:
  ```rust
  struct TranslationMemory {
      entries: VecDeque<(String, String)>,  // (japanese, english)
      max_size: usize,                       // default: 6
  }
  ```
- [ ] `fn push(&mut self, japanese: String, english: String)` — adds entry, evicts oldest if over `max_size`
- [ ] `fn clear(&mut self)` — wipes all entries
- [ ] `fn as_context_slice(&self) -> &[(String, String)]` — returns entries for prompt injection
- [ ] After each successful translation batch: push all `(original, translated)` pairs to memory
- [ ] Test: Translate two sequential screens; verify second screen resolves dropped subject from first screen's context

### 3.4 Context Invalidation Strategy

> **Correction from earlier drafts:** The auto-clear mechanism is NOT based on ScreenCaptureKit frame metadata. It uses `NSWorkspaceDidActivateApplicationNotification`, which is the correct and reliable macOS API for detecting the frontmost app changing.

- [ ] Add `InvalidationReason` enum:
  ```rust
  enum InvalidationReason {
      AppSwitch { from: String, to: String },
      ManualReset,
      ModelSwitch,
  }
  ```
- [ ] Create a `crossbeam` channel: `invalidation_tx / invalidation_rx`; pass `invalidation_rx` to the component that owns `TranslationMemory`
- [ ] Write an `AppWindowTracker` struct:
  - [ ] `current_bundle_id: Option<String>` — tracks the last known frontmost app
  - [ ] On init: query `NSWorkspace.shared.frontmostApplication` to populate initial value
  - [ ] Subscribe to `NSWorkspaceDidActivateApplicationNotification` via `objc2-app-kit`:
    - [ ] In the notification callback: read the new app's `bundleIdentifier`
    - [ ] Compare to `current_bundle_id`
    - [ ] If different: update `current_bundle_id`, send `InvalidationReason::AppSwitch` on channel
  - [ ] **Fallback:** If notification subscription fails (sandbox restriction), poll `NSWorkspace.shared.frontmostApplication` every 2 seconds in a background thread; log a warning at startup
- [ ] In the translation pipeline's main loop: drain `invalidation_rx` before each translation cycle:
  - [ ] `AppSwitch` received → call `translation_memory.clear()` + emit `"translation-clear"` to frontend + log
  - [ ] `ManualReset` received → call `translation_memory.clear()` only (do NOT clear overlay — user may be reading)
  - [ ] `ModelSwitch` received → call `translation_memory.clear()` + emit `"translation-clear"` to frontend
- [ ] Wire `Cmd+Shift+M` hotkey → send `InvalidationReason::ManualReset` on `invalidation_tx`
- [ ] Wire model switch logic (Phase 3.5) → send `InvalidationReason::ModelSwitch` on `invalidation_tx`

**Testing checklist:**

- [ ] Open Safari with Japanese content → trigger translation → switch to Terminal → switch back to Safari → verify memory was cleared on both switches
- [ ] Open two Safari tabs with Japanese content → switch between tabs → verify memory is **not** cleared (same app, same bundle ID)
- [ ] Press `Cmd+Shift+M` mid-session → verify memory clears but overlay boxes remain visible
- [ ] Switch models with `Cmd+Shift+G` → verify memory clears and overlay clears
- [ ] Log output during app switch shows: `[ContextInvalidation] AppSwitch: com.apple.Safari → com.apple.dt.Xcode — memory cleared`

### 3.4 Gemma 4 E4B Quality Mode

- [ ] Download `gemma-4-e4b-it.Q4_K_M.gguf` from Hugging Face (~5GB)
- [ ] Add to `manifest.json` with `"active": false` by default
- [ ] Write `fn switch_model(new_model_id: &str) -> Result<()>`:
  - [ ] Unload current model (drop + deallocate GPU buffers)
  - [ ] Show "Loading model…" indicator in overlay/tray
  - [ ] Load new model
  - [ ] Clear translation memory on switch (context from different model is not reliable)
- [ ] Register `Cmd+Shift+G` hotkey to toggle between NLLB and Gemma 4 E4B
- [ ] Before switching to Gemma 4: check available RAM; if system RAM < 8GB free, warn user
- [ ] Update tray menu to show active model name and a toggle option
- [ ] Test: Switch models mid-session; verify overlay continues working with new model

### 3.5 Parallel Styling (CPU Tasks Only)

- [ ] Use `rayon::par_iter()` for CPU-bound work that IS safe to parallelize:
  - [ ] Background color sampling (one thread per bounding box)
  - [ ] Contrast luminance calculation
  - [ ] IPC payload serialization
- [ ] Do **not** parallelize inference

### 3.6 Translation Quality Validation

- [ ] Create a test set of 20 Japanese sentences with known English translations
- [ ] Test 5 sentences that require prior context to translate correctly (dropped subjects, pronouns)
- [ ] Run tests on both NLLB and Gemma 4 E4B; compare output quality
- [ ] If NLLB quality is insufficient even with context, document this in `DECISIONS.md`

**✅ Phase 3 Milestone:** Given a list of 5 Japanese strings, NLLB returns translations in under 3 seconds. Rolling memory correctly carries context across two sequential screen triggers. Model switch between NLLB and Gemma 4 works without crash.

---

## Phase 4 — Dynamic Styling

_Goal: Calculate readable text/background colors for each translation box._

### 4.1 Background Color Sampling

- [ ] Write `fn sample_border_color(buffer: &PixelBuffer, rect: ScreenRect) -> Rgba`
  - [ ] Sample the outer 2px ring of pixels around the bounding box
  - [ ] Average all sampled RGBA values
  - [ ] Clamp rect to screen bounds before sampling

### 4.2 Contrast Calculation

- [ ] Implement WCAG 2.1 relative luminance formula:
  ```
  fn relative_luminance(r: f32, g: f32, b: f32) -> f32
  fn linearize_channel(c: f32) -> f32  // sRGB to linear
  ```
- [ ] Determine foreground color:
  - `L > 0.179` → `fg_color = "#000000"`
  - `L ≤ 0.179` → `fg_color = "#FFFFFF"`
- [ ] Compute overlay background: sampled color with 85% opacity
  - Format as `rgba(r, g, b, 0.85)`

### 4.3 Unit Tests

- [ ] Test: White background → black text
- [ ] Test: Black background → white text
- [ ] Test: Mid-gray (#808080) → correct threshold behavior
- [ ] Test: Colored background (e.g., dark blue) → white text

**✅ Phase 4 Milestone:** Color sampling and contrast logic pass all unit tests.

---

## Phase 5 — IPC & Frontend Rendering

_Goal: Render translated text as perfectly-positioned overlay boxes in the WebView._

### 5.1 IPC Payload Assembly

- [ ] Define `TranslationBox` struct with `#[derive(Serialize)]`:
  ```rust
  struct TranslationBox {
      id: String,
      translated: String,
      original: String,
      x: f32, y: f32,
      width: f32, height: f32,
      bg_color: String,
      fg_color: String,
      confidence: f32,
  }
  ```
- [ ] Define `TranslationPayload` struct: `{ boxes: Vec<TranslationBox>, scale_factor: f32, frame_id: u64 }`
- [ ] Write `fn build_payload(ocr: Vec<OcrResult>, translations: Vec<String>, ...) -> TranslationPayload`

### 5.2 Tauri Event Emission

- [ ] In Tauri command handler, emit `"translation-update"` with serialized payload
- [ ] Emit `"translation-clear"` when `DebounceEvent::MotionDetected` fires
- [ ] Test: Print events in browser DevTools console (`Ctrl+Shift+I` in Tauri dev mode)

### 5.3 Frontend HTML/CSS/JS

- [ ] Set `<body>` to `margin: 0; padding: 0; overflow: hidden; background: transparent`
- [ ] Create `#overlay` container: `position: fixed; top: 0; left: 0; width: 100vw; height: 100vh; pointer-events: none`
- [ ] Write `translation-update` event listener:
  - [ ] Clear all existing `.translation-box` elements
  - [ ] For each box, create and append absolutely-positioned `<div>`
  - [ ] Apply dynamic inline styles (position, colors, font size)
- [ ] Write `translation-clear` event listener: remove all `.translation-box` elements
- [ ] Add CSS transition for smooth box appearance: `opacity 0.15s ease-in`

### 5.4 Visual Alignment Testing

- [ ] Open a Japanese webpage in Safari/Chrome
- [ ] Trigger a translation and verify boxes align with original text
- [ ] Test at 1x scale (external monitor) and 2x scale (Retina MacBook display)
- [ ] Fix any Y-axis or scale_factor misalignment

**✅ Phase 5 Milestone:** Translated English text appears over Japanese text on a real webpage, correctly aligned, with readable colors.

---

## Phase 6 — Global Hotkeys & App Polish

_Goal: Full UX control — toggle, quit, force-retranslate._

### 6.1 Global Shortcuts

- [ ] Add `tauri-plugin-global-shortcut` to project
- [ ] Register `Cmd+Shift+T` → toggle overlay visibility
- [ ] Register `Cmd+Shift+Q` → quit application
- [ ] Register `Cmd+Shift+R` → bypass debounce, force OCR + translation immediately
- [ ] Register `Cmd+Shift+M` → manually clear translation memory (context reset)
- [ ] Register `Cmd+Shift+G` → toggle between NLLB and Gemma 4 E4B quality mode
- [ ] Test: Shortcuts work when Tauri window is not focused (browser is in front)

### 6.2 System Tray / Menu Bar Icon

- [ ] Add `tauri-plugin-tray` or use Tauri v2 tray API
- [ ] Add menu bar icon (simple "JP" or translation icon)
- [ ] Menu items:
  - [ ] "Enable / Disable Overlay" (toggle)
  - [ ] "Translate Now" (force retrigger)
  - [ ] "Settings" (placeholder for v1.1)
  - [ ] "Quit"

### 6.3 Startup & First-Run Experience

- [ ] On first launch, check for screen recording permission
  - [ ] If missing, open System Settings → Privacy → Screen Recording
- [ ] On first launch, check for model file in Application Support
  - [ ] If missing, show a dialog with download instructions and model path
- [ ] Show a brief "Overlay Active" notification on startup (macOS `NSUserNotification`)

### 6.4 Logging

- [ ] Set up `env_logger` with `RUST_LOG=info` default
- [ ] Log all state machine transitions at `debug` level
- [ ] Log OCR trigger events, result counts, and translation durations at `info` level
- [ ] Log errors to `~/Library/Logs/jp-translate/app.log`

**✅ Phase 6 Milestone:** App can be toggled on/off via hotkey and tray menu. First-run flow guides user through permissions.

---

## Phase 7 — Performance Optimization & Hardening

_Goal: Hit latency and memory targets; handle edge cases._

### 7.1 Performance Profiling

- [ ] Profile with Xcode Instruments: Time Profiler + Allocations
- [ ] Identify top CPU hotspots in the motion detection loop
- [ ] Measure end-to-end latency: frame capture → overlay render
- [ ] Measure peak RAM usage; verify it stays under 3GB

### 7.2 Optimization Tasks

- [ ] If motion detection is slow: pre-allocate thumbnail buffers, avoid heap allocation in hot path
- [ ] If translation is slow: experiment with smaller batch sizes, or reduce `n_ctx` in llama.cpp
- [ ] If overlay render is janky: batch DOM updates using `requestAnimationFrame`
- [ ] Test on a cold start: verify model load time is under 5 seconds

### 7.3 Edge Case Hardening

- [ ] Handle display resolution changes mid-session (update scale_factor + stream config)
- [ ] Handle system sleep/wake: restart ScreenCaptureKit stream on wake
- [ ] Handle rapid app switching: ensure overlay clears when the screen content changes
- [ ] Handle OCR returning empty results: ensure overlay clears gracefully
- [ ] Test with mixed Japanese/English text: verify English passthrough (don't translate English)

### 7.4 Memory Leak Check

- [ ] Run app for 30 minutes; monitor memory usage in Activity Monitor
- [ ] Verify no unbounded growth in frame buffer allocations
- [ ] Verify no WebView DOM node accumulation over many translation cycles

**✅ Phase 7 Milestone:** End-to-end latency < 2s for a typical 5-element Japanese webpage. Memory stable after 30 minutes.

---

## Phase 8 — Build, Sign & Distribution

_Goal: A distributable .app bundle that installs and runs cleanly._

### 8.1 Code Signing

- [ ] Configure Tauri signing in `tauri.conf.json` with Apple Developer certificate
- [ ] Set bundle identifier: `com.yourname.jp-translate`
- [ ] Add required entitlements to `entitlements.plist`

### 8.2 Notarization

- [ ] Set up `xcrun notarytool` with App Store Connect API key
- [ ] Add notarization step to build script
- [ ] Test: Install notarized `.dmg` on a clean Mac; verify no Gatekeeper warnings

### 8.3 Packaging

- [ ] Bundle model file inside `.app` or document the manual copy step for users
  - Prefer: ship without model, prompt user to download (smaller initial download)
- [ ] Create a `.dmg` installer via Tauri's bundler
- [ ] Write a `README.md` with installation and first-run instructions

### 8.4 Final QA Checklist

- [ ] Test on macOS 13 Ventura (minimum target)
- [ ] Test on macOS 14 Sonoma
- [ ] Test on MacBook Pro (Retina, 2x scale)
- [ ] Test on external 4K monitor (1x scale)
- [ ] Test with Safari, Chrome, Firefox (different rendering backends)
- [ ] Test with PDF viewer, terminal, VS Code

**✅ Phase 8 Milestone:** A `.dmg` installs cleanly, passes Gatekeeper, and the full translation pipeline works on a fresh machine.

---

## Backlog / Future Versions (v1.1+)

- [ ] Settings UI: adjust debounce timing, motion threshold, font size, memory window size
- [ ] Support for window-specific capture (translate only one app)
- [ ] Persist translation memory across sessions (opt-in, per-app)
- [ ] Vocabulary lookup on hover (tap a translation box to see dictionary entry)
- [ ] Export translations to clipboard or text file
- [ ] Support Chinese (Traditional/Simplified) and Korean
- [ ] Auto-update mechanism

---

## Quick Reference: Key File Locations

```
jp-translate/
├── src-tauri/
│   ├── src/
│   │   ├── main.rs              # Tauri app entry point
│   │   ├── capture.rs           # ScreenCaptureKit integration
│   │   ├── motion.rs            # Delta check + debounce state machine
│   │   ├── ocr.rs               # Apple Vision OCR engine
│   │   ├── translation.rs       # llama.cpp translation engine
│   │   ├── styling.rs           # Color sampling + contrast calc
│   │   └── ipc.rs               # Payload types + Tauri event emitter
│   ├── Cargo.toml
│   ├── tauri.conf.json
│   └── entitlements.plist
├── src/
│   ├── index.html               # Overlay WebView
│   ├── overlay.js               # Event listeners + DOM rendering
│   └── overlay.css              # Transparent overlay styles
├── SPEC.md
├── TODO.md
└── README.md
```
