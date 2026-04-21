# SPEC.md — Real-Time Screen Translation Overlay

**Version:** 1.3.0
**Target Platform:** macOS 13+ (Apple Silicon, M-series)
**Last Updated:** 2026-04-21

---

## Changelog

| Version | Changes                                                                                                                                                                                                                                                   |
| ------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1.0.0   | Initial specification                                                                                                                                                                                                                                     |
| 1.1.0   | Vertical text, furigana suppression, multi-monitor, crash recovery, thermal awareness, onboarding wizard, Sentry                                                                                                                                          |
| 1.2.0   | Consolidated duplicate content; fixed section numbering; added spinner feedback, settings.json, RAM guard, privacy screen, in-app help, `--test-suite` CLI flag                                                                                           |
| 1.3.0   | Reflects current scaffolded project state; resolves three open architecture decisions (capture, OCR, translation); adds `llama-server` sidecar as translation implementation; adds Apple Foundation Models as Tier 3; adds Pipeline Orchestration section |

---

## Implementation Status (as of v1.3.0)

| Phase   | Description                                              | Status                                                                          |
| ------- | -------------------------------------------------------- | ------------------------------------------------------------------------------- |
| Phase 0 | Project scaffold, dependencies, CLI flags, settings.json | ✅ Done                                                                         |
| Phase 1 | ScreenCaptureKit capture + motion detection              | 🔨 Scaffolded — mocked; real SCKit capture not yet wired                        |
| Phase 2 | Apple Vision OCR + furigana suppression                  | 🔨 Scaffolded — `vision-helper` binary approach decided; not yet invoked        |
| Phase 3 | Translation engine + context memory + model management   | 🔨 Scaffolded — mock uppercase translation in place; llama-server not yet wired |
| Phase 4 | Dynamic styling (WCAG contrast)                          | ✅ Done — pure math, no FFI dependencies                                        |
| Phase 5 | IPC payload + frontend rendering                         | ✅ Done — structure correct; fires with mock data until pipeline is real        |
| Phase 6 | Hotkeys, tray, first-run wizard, help, auto-update       | ✅ Done                                                                         |
| Phase 7 | E2E test suite + performance hardening                   | 🔨 Scaffolded — framework exists; cannot pass with mock pipeline                |
| Phase P | Pipeline Activation — wire real implementations          | ✅ Done                                                                         |
| Phase 8 | Code signing, notarization, distribution                 | ⬜ Current focus                                                                |

**The core task right now is Phase P: replacing every mock/boilerplate with real implementations and wiring them together in `main.rs`.**

---

## 1. Project Overview

A high-performance desktop overlay application that detects Japanese text on-screen, translates it to English in real-time using local AI models, and renders translated text as a transparent, click-through overlay precisely positioned over the original text. The application runs entirely offline after initial model download and is designed to be installable by non-technical users.

---

## 2. Goals & Non-Goals

### Goals

- Translate Japanese text visible in any macOS application (browsers, PDFs, video players, manga readers, etc.)
- Maintain real-time responsiveness with sub-2-second end-to-end latency after the debounce trigger
- Keep total RAM footprint under 4GB (standard mode) / 8GB (quality mode) to prevent SSD swapping
- Operate without any internet connection after setup
- Be thermally responsible — no continuous GPU/ANE hammering
- Provide a polished first-run onboarding experience for non-technical users

### Non-Goals

- Translating audio or video subtitles in real-time (frame-by-frame)
- Supporting languages other than Japanese to English in v1.0
- Providing a history, clipboard, or dictionary lookup feature in v1.0
- Running on Intel Macs or non-Apple hardware

---

## 3. System Architecture

```
+----------------------------------------------------------------+
|                      TAURI APPLICATION                         |
|                                                                |
|  +-------------------------+   +----------------------------+  |
|  |      Rust Backend        |   |     WebView Frontend       |  |
|  |                          |   |     (HTML/CSS/JS)          |  |
|  |  +------------------+   |   |                            |  |
|  |  | Swift: capture    |   |   |  Multiple Transparent      |  |
|  |  |  helper OR        |   |   |  Overlay Windows           |  |
|  |  |  screencapturekit |   |   |  (one per display)         |  |
|  |  +--------+---------+   |   |  Absolutely-positioned     |  |
|  |           |              |   |  <div> translation boxes   |  |
|  |  +--------v---------+   |   |  (horizontal & vertical)   |  |
|  |  |  Motion Delta     |   |   +----------------------------+  |
|  |  |  Detector (Rust)  |   |              ^                    |
|  |  +--------+---------+   |              | Tauri IPC          |
|  |           |              |              | (JSON payload)     |
|  |  +--------v---------+   |              |                    |
|  |  | Swift: vision-    +---+--------------+                   |
|  |  |  helper (subprocess)  |                                   |
|  |  +--------+---------+   |                                   |
|  |           |              |                                   |
|  |  +--------v---------+   |                                   |
|  |  | llama-server      |   |                                   |
|  |  |  sidecar (HTTP)   |   |                                   |
|  |  |  OR Foundation    |   |                                   |
|  |  |  Models (macOS 26)|   |                                   |
|  |  +--------+---------+   |                                   |
|  |           |              |                                   |
|  |  +--------v---------+   |                                   |
|  |  |  Thermal/Battery  |   |                                   |
|  |  |  Monitor (IOKit)  |   |                                   |
|  |  +------------------+   |                                   |
|  +-------------------------+                                   |
+----------------------------------------------------------------+
```

---

## 4. Technology Stack

| Layer                     | Technology                                            | Version Target   | Justification                                                                  |
| ------------------------- | ----------------------------------------------------- | ---------------- | ------------------------------------------------------------------------------ |
| App Framework             | Tauri                                                 | v2.x             | Lightweight, native OS integration, IPC, transparent windows                   |
| Backend Language          | Rust                                                  | 1.78+ (stable)   | Memory safety, zero-cost abstractions, macOS FFI                               |
| Screen Capture            | `screencapturekit` Rust crate OR Swift capture helper | macOS 13+        | See Section 5.1 — decision gate                                                |
| OCR                       | Swift `vision-helper` subprocess                      | macOS 13+        | **Decided.** Avoids unstable `objc2-vision` FFI; 5ms spawn overhead acceptable |
| Translation (Default)     | `llama-server` sidecar binary                         | latest llama.cpp | **Decided.** Pre-compiled; Tauri manages lifecycle; OpenAI-compatible HTTP API |
| Translation (Quality)     | `llama-server` sidecar — Gemma 4 E4B IT (Q4_K_M)      | —                | Same sidecar, different model loaded                                           |
| Translation (Native Tier) | Apple Foundation Models framework                     | macOS 26+        | Zero RAM cost, ANE-accelerated; Swift subprocess bridge; v1.1 feature          |
| Parallelism               | Rayon                                                 | latest           | CPU-bound tasks only (styling, coordinate math, serialization)                 |
| Frontend                  | HTML5 / CSS3 / Vanilla JS                             | —                | Minimal, no framework overhead for overlay rendering                           |
| Crash Reporting           | sentry-rust (opt-in)                                  | latest           | Anonymous crash telemetry, explicit user consent                               |
| HTTP Client               | reqwest                                               | latest           | In-app model downloader + llama-server health checks                           |

---

## 5. Subsystem Specifications

### 5.1 Screen Capture — ScreenCaptureKit

**Purpose:** Continuously capture the contents of each display into pixel buffers.

**Architecture Decision — Resolved:**

| Approach                        | Pros                                            | Cons                                                                                       | Decision                       |
| ------------------------------- | ----------------------------------------------- | ------------------------------------------------------------------------------------------ | ------------------------------ |
| `screencapturekit` Rust crate   | Single process, lowest latency, no IPC overhead | Crate may have incomplete bindings; real-time frame delivery via Rust callbacks is complex | **Preferred if bindings work** |
| Swift capture helper subprocess | Stable, uses native Swift SCStream API directly | Subprocess for real-time pixel data adds IPC overhead; harder to implement                 | Use as fallback only           |

**Recommendation:** Try the `screencapturekit` Rust crate first (time-box to 1 day). If frame delivery or `CVPixelBuffer` access is broken, pivot to a lightweight Swift helper that pipes frames over a Unix domain socket or writes to a shared memory region.

**Implementation Details:**

- Enumerate all `NSScreen.screens()` on startup
- For each display: one `SCStream` with `SCContentFilter`, `BGRA8Unorm` pixel format, 30 FPS
- Exclude the overlay window via `excludedWindows` to prevent capture loop
- Deliver frames to Rust via a bounded `crossbeam` channel (capacity 2, drop on full — backpressure)

**Entitlements Required:**

```xml
<key>com.apple.security.screen-capture</key>
<true/>
```

**Multi-Monitor Behavior:**

- One Tauri overlay window per display, sized to logical frame
- Each overlay is click-through (`set_ignore_cursor_events(true)`)
- Translation payloads carry `display_id` for routing

**Display Hot-Plug Handling:**

- Subscribe to `CGDisplayRegisterReconfigurationCallback`
- On display removed: stop `SCStream`, drop channel, close Tauri window
- On display added: create new stream and overlay window

**Threading Model:**

- One frame-processing thread per display
- Threads share a single `TranslationClient` instance (HTTP client to `llama-server`)

---

### 5.2 Motion Detection (Delta Check)

**Status:** Scaffolded — logic is implemented but receives mock frames. Will become real once Section 5.1 is wired.

**Algorithm:**

```
1. Receive frame (CMSampleBuffer -> CVPixelBuffer)
2. Downscale to 160x90 grayscale thumbnail (bilinear interpolation)
3. Exclude 5% inset margin from all four edges
4. Compute per-pixel absolute differences vs. previous thumbnail
5. Build binary changed-pixel mask (changed if diff > PIXEL_DIFF_THRESHOLD = 15)
6. Run connected-components pass (4-connectivity union-find)
7. Find largest single connected region
8. motion_ratio = largest_region_area / total_comparison_area
9. motion_ratio > 0.05 -> "motion detected"
```

**Why connected-components:** Spinning loaders, blinking cursors, and ad animations produce scattered isolated pixels. Scrolling produces one large cohesive block. This prevents false debounce resets from UI noise.

**Debounce State Machine:**

```
SCROLLING:
  - motion > threshold  -> stay SCROLLING, reset timer to 300ms, hide overlay
  - motion <= threshold -> transition to SETTLING

SETTLING:
  - motion > threshold  -> back to SCROLLING
  - timer hits 0        -> IDLE, snap frame, trigger OCR pipeline

IDLE:
  - motion > threshold  -> back to SCROLLING, hide overlay
  - no motion           -> stay IDLE
```

**Constants (configurable via `settings.json`):**

| Constant               | Default | Description                                        |
| ---------------------- | ------- | -------------------------------------------------- |
| `MOTION_THRESHOLD`     | 0.05    | Fraction of largest connected changed-pixel region |
| `PIXEL_DIFF_THRESHOLD` | 15      | Per-pixel grayscale diff to count as changed       |
| `DEBOUNCE_MS`          | 300     | Milliseconds of stillness before OCR trigger       |
| `EDGE_INSET_PERCENT`   | 5       | % of screen edge excluded from comparison          |
| `CAPTURE_FPS`          | 30      | Target frame capture rate                          |

---

### 5.3 OCR — Apple Vision via Swift Subprocess

**Status:** Scaffolded — `OcrEngine` struct exists with mock output. `vision-helper` binary needs to be built and invoked.

**Architecture Decision — Resolved: Swift subprocess.**

After evaluating `objc2-vision`, writing raw unsafe Objective-C FFI bindings for `VNRecognizeTextRequest` is high-risk and time-consuming. The Swift subprocess approach is production-viable: ~5ms spawn overhead is negligible given the 300ms debounce gate, and the resulting binary is stable and maintainable.

**`vision-helper` Swift CLI tool:**

```
Location: src-tauri/src/bin/vision-helper.swift
Compiled output: bundled inside .app at Resources/vision-helper
```

**Input:** path to a PNG file (saved temporarily from the pixel buffer snapshot)
**Output:** JSON array to stdout:

```json
[
  {
    "text": "日本語のテキスト",
    "confidence": 0.97,
    "x": 0.12,
    "y": 0.45,
    "width": 0.3,
    "height": 0.04,
    "text_angle": 0.0
  }
]
```

**Swift implementation uses:**

- `VNRecognizeTextRequest` with `recognitionLevel = .accurate`
- `recognitionLanguages = ["ja-JP"]`
- `usesLanguageCorrection = true`
- `textAngle` from `VNRecognizedTextObservation`

**Rust `OcrEngine.recognize()` calls:**

```rust
let child = Command::new(vision_helper_path)
    .arg(temp_png_path)
    .output()?;
let results: Vec<VisionHelperResult> = serde_json::from_slice(&child.stdout)?;
```

**Post-processing in Rust (unchanged from previous spec):**

- Derive `is_vertical = |text_angle| > π/4`
- Convert Vision coordinates (bottom-left, normalized) to screen coordinates
- Apply furigana suppression (proximity-based)
- Filter: `confidence < 0.4`, no CJK characters, merge overlapping boxes (IoU > 0.3)

**Output per recognized region:**

```rust
struct OcrResult {
    text: String,
    confidence: f32,
    bounding_box: ScreenRect,  // already in logical CSS points
    is_vertical: bool,
}
```

---

### 5.4 Translation Engine — llama-server Sidecar

**Status:** Scaffolded — `TranslationEngine` struct exists with mock uppercase. `llama-server` sidecar not yet bundled or managed.

**Architecture Decision — Resolved: `llama-server` sidecar.**

Compiling `llama.cpp` directly into the Rust binary via `llama-cpp-rs` requires building llama.cpp with Metal support inside the Tauri build system — significant setup, brittle cross-compilation, and hard to update. Instead, the pre-compiled `llama-server` binary from the official llama.cpp project is bundled as a **Tauri sidecar**. Rust communicates with it via a local HTTP port using the OpenAI-compatible API.

**Benefits:**

- `llama-server` is an official, tested binary from the llama.cpp project
- Metal support is compiled into the official release binary — no custom build needed
- Tauri sidecar support handles process lifecycle (start on app launch, kill on quit)
- `llama-server` can be updated independently of the Rust code
- OpenAI-compatible HTTP API is well-documented and easy to call from Rust via `reqwest`

**Sidecar Configuration:**

```toml
# tauri.conf.json
{
  "bundle": {
    "externalBin": ["binaries/llama-server"]
  }
}
```

**Process Lifecycle:**

```rust
// Tauri manages the sidecar; Rust holds the child handle
let sidecar = app.shell().sidecar("llama-server")?
    .args([
        "--model", model_path,
        "--port", "8765",
        "--n-gpu-layers", "99",      // full Metal offload
        "--ctx-size", "1024",
        "--host", "127.0.0.1",
        "--log-disable",             // quiet; Rust handles logging
    ])
    .spawn()?;
```

**Health Check:** On startup, poll `GET http://127.0.0.1:8765/health` until `{"status": "ok"}` is returned (max 10s timeout, then show error).

**Translation API Call:**

```rust
// POST http://127.0.0.1:8765/v1/chat/completions
{
  "model": "local",
  "messages": [
    { "role": "system", "content": "You are a Japanese-to-English translator..." },
    { "role": "user", "content": "<batched numbered prompt with context>" }
  ],
  "temperature": 0.1,
  "max_tokens": 512
}
```

**Translation Request Format — Batched Single-Pass with Rolling Context:**

```
[If context memory non-empty:]
Previous context (do not retranslate, for reference only):
- {memory[0].ja} -> "{memory[0].en}"
...up to 6 entries

Translate each numbered Japanese string to English.
Output only translations, one per line, same numbered format.

1: {ocr_result[0].text}
2: {ocr_result[1].text}
...N: {ocr_result[N].text}
```

**Output parsing:**

- Match `^(\d+): (.+)$` per response line
- Map back to original `OcrResult` by index
- `""` for missing/malformed lines → overlay shows original Japanese as fallback
- If OCR returns > 15 strings: split into sequential sub-batches of 15

#### Model Tiers

| Tier              | Model                   | Size (Q4_K_M)  | RAM    | Minimum OS |
| ----------------- | ----------------------- | -------------- | ------ | ---------- |
| **Default**       | NLLB-200-distilled-600M | ~1.2 GB        | Low    | macOS 13+  |
| **Quality Mode**  | Gemma 4 E4B IT          | ~5 GB          | Higher | macOS 13+  |
| **Native (v1.1)** | Apple Foundation Models | ~0 GB (system) | None   | macOS 26+  |

The active model is user-switchable via tray or `Cmd+Shift+G`. On switch, the `llama-server` sidecar is restarted with the new `--model` path.

**RAM Gate:** At startup, `sysctl hw.memsize`. If total RAM < 12GB, Quality Mode is disabled entirely.

#### Performance Targets

| Metric                              | NLLB (Default) | Gemma 4 (Quality) |
| ----------------------------------- | -------------- | ----------------- |
| Translation latency (single string) | < 800ms        | < 1.5s            |
| Translation latency (~10 strings)   | < 3s           | < 5s              |
| Sidecar startup time                | < 5s           | < 8s              |
| Peak RAM (model + app)              | < 2GB          | < 6.5GB           |

#### Crash Recovery Watchdog

A watchdog polls `GET /health` every 5 seconds. If three consecutive polls fail:

1. Restart `llama-server` sidecar with same arguments
2. Emit `"translation-error"` → frontend banner: _"Translation engine restarted."_ (4s auto-dismiss)
3. Clear overlay until first successful translation response

#### Thermal / Battery Awareness

Subscribe to IOKit power source and thermal notifications. On battery + thermal state `serious` or `critical`:

- Restart sidecar with NLLB model (if Gemma 4 active); send `ModelSwitch` invalidation
- Increase `DEBOUNCE_MS` to 600 in runtime state
- Show thermal badge on tray icon

---

### 5.5 Apple Foundation Models — Native Tier (v1.1)

**Status:** Not started. Deferred to v1.1. Architecture documented here for planning.

**Purpose:** On macOS 26+ with Apple Intelligence enabled, replace `llama-server` with a zero-RAM-cost on-device translation call via Foundation Models framework. Japanese is a supported language.

**Implementation approach:** A Swift helper binary (`translation-helper`) similar to `vision-helper`, called from Rust via `std::process::Command`. The helper uses `FoundationModels.LanguageModelSession` to run inference and returns translated strings as JSON to stdout.

**Auto-detection:** On startup, check macOS version and Apple Intelligence availability. If macOS ≥ 26 and Foundation Models is available, set `native_tier_available = true` and offer it as a third option in the tray menu.

**Tradeoffs vs. llama-server:**

- Zero model download required — model already on device
- Zero RAM overhead — shares the system Apple Intelligence model
- No model management needed
- Con: Guardrails may produce false positives on mature manga content
- Con: No control over model updates (Apple updates silently with OS)
- Con: Requires Apple Intelligence enabled by user in System Settings

---

### 5.6 Pipeline Orchestration (`main.rs`)

**Status:** ⬜ Not yet wired. This is the primary task of Phase P.

**Purpose:** The `main.rs` setup closure must spawn and connect all subsystem threads into a working pipeline. Currently, modules are imported but the engine is annotated `#[expect(dead_code)]` and detached.

**Required wiring:**

```
setup() {
    1. Start llama-server sidecar; wait for /health
    2. Load settings.json
    3. Initialize AppWindowTracker (subscribe NSWorkspaceDidActivateApplicationNotification)
    4. Initialize TranslationMemory
    5. Subscribe IOKit thermal notifications
    6. For each display:
       a. Create SCStream (or launch capture helper)
       b. Create frame channel (capacity 2)
       c. Spawn frame-processing thread:
          - MotionDetector
          - DebounceStateMachine
          - On Triggered:
              i.   Save frame snapshot as temp PNG
              ii.  Invoke vision-helper with PNG path
              iii. Parse OcrResult vec
              iv.  Apply furigana suppression + filtering
              v.   Drain invalidation_rx; apply any pending clears
              vi.  Call TranslationClient.translate_batch(strings, context)
              vii. Apply styling (Rayon par_iter for color sampling)
              viii.Build TranslationPayload
              ix.  Emit "translation-update" to correct overlay window
              x.   Push results to TranslationMemory
          - On MotionDetected:
              Emit "translation-clear" to correct overlay window
    7. Subscribe CGDisplayRegisterReconfigurationCallback for hot-plug
}
```

---

### 5.7 Rolling Translation Memory

**Status:** Scaffolded — struct and methods exist.

**State:**

```rust
struct TranslationMemory {
    entries: VecDeque<(String, String)>,  // (japanese, english)
    max_size: usize,                       // from settings.json, default 6
}
```

**Invalidation rules:**

- On `translation-clear` (scroll): keep memory
- On app switch (`NSWorkspaceDidActivateApplicationNotification`): auto-clear
- On `Cmd+Shift+M`: manual clear (keep overlay visible)
- On model switch: auto-clear
- Browser tab switches (same bundle ID): do NOT clear

---

### 5.8 Context Invalidation Strategy

**Status:** Scaffolded — `AppWindowTracker` and `InvalidationReason` enum exist.

**Mechanism:** `NSWorkspaceDidActivateApplicationNotification` (not ScreenCaptureKit metadata — SCKit does not expose per-frame frontmost app reliably).

```rust
enum InvalidationReason {
    AppSwitch { from: String, to: String },
    ManualReset,
    ModelSwitch,
}
```

**Trigger Matrix:**

| Trigger       | Clears Memory | Clears Overlay |
| ------------- | ------------- | -------------- |
| App switch    | Yes           | Yes            |
| `Cmd+Shift+M` | Yes           | No             |
| Model switch  | Yes           | Yes            |
| Scroll/motion | No            | Yes            |

**Fallback:** If notification subscription fails, poll `NSWorkspace.shared.frontmostApplication` every 2 seconds. Log warning.

---

### 5.9 Dynamic Styling

**Status:** ✅ Done — pure math, no FFI.

**Algorithm:**

```
1. Sample outer 2px ring of bounding box from pixel buffer -> avg RGBA
2. WCAG 2.1 relative luminance:
   L = 0.2126*linearize(r) + 0.7152*linearize(g) + 0.0722*linearize(b)
   linearize(c) = c/12.92 if c<=0.04045 else ((c+0.055)/1.055)^2.4
3. L > 0.179  -> fg = "#000000"
   L <= 0.179 -> fg = "#FFFFFF"
4. overlay_bg = sampled color at 85% opacity
```

---

### 5.10 IPC Payload — Rust to Tauri Frontend

**Status:** ✅ Done — structure correct; currently fires with mock data.

```typescript
interface TranslationBox {
  id: string;
  translated: string;
  original: string;
  x: number;
  y: number;
  width: number;
  height: number;
  is_vertical: boolean;
  bg_color: string;
  fg_color: string;
  confidence: number;
}

type TranslationPayload = {
  boxes: TranslationBox[];
  scale_factor: number;
  display_id: number;
  frame_id: number;
};
```

**Events:**

| Event                   | When                       |
| ----------------------- | -------------------------- |
| `"translation-update"`  | New batch ready            |
| `"translation-clear"`   | Motion or app switch       |
| `"translation-started"` | Inference begun            |
| `"translation-error"`   | Watchdog restarted sidecar |

---

### 5.11 Tauri Overlay Window

**Status:** ✅ Done.

```json
{
  "transparent": true,
  "decorations": false,
  "alwaysOnTop": true,
  "resizable": false,
  "skipTaskbar": true,
  "shadow": false
}
```

```rust
window.set_ignore_cursor_events(true)?;  // click-through
window.set_content_protection(false)?;   // allow SCKit capture
```

---

## 6. Model Download & First-Run Onboarding

**Status:** ✅ Done — 4-screen wizard implemented.

### 6.1 Onboarding Wizard (4 Screens)

**Screen 1:** Screen recording permission — poll until granted
**Screen 2:** Model selection (Standard/Quality; Quality greyed if RAM < 12GB)
**Screen 3:** In-app download with resumable HTTP Range + SHA256 verify; background download if wizard closed
**Screen 4:** Privacy disclosure + opt-in Sentry checkbox

### 6.2 Model Management

**Storage:** `~/Library/Application Support/jp-translate/models/`
**Manifest:** `models/manifest.json` — tracks `id`, `filename`, `size_bytes`, `sha256`, `downloaded_at`, `last_used_at`, `active`

**Rules:**

- Scan for orphan `.gguf` files on startup; offer deletion
- Non-active model unused > 30 days: prompt to prune
- Never silently delete
- Hard 4GB ceiling; block downloads if exceeded

**Also required:** `llama-server` binary needs to be bundled in the app (not downloaded per-user — it ships with the `.app`). This is distinct from the model files.

---

## 7. Settings File

**Status:** ✅ Done.

**Path:** `~/Library/Application Support/jp-translate/settings.json`

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

---

## 8. Debug CLI Mode

**Status:** ✅ Scaffolded — structure works; outputs mock data until pipeline is real.

**Activation:** `cargo run -- --debug-cli`

Bypasses Tauri window creation. Runs full Rust engine and prints JSON to stdout per debounce trigger. Once Phase P is complete, this will output real OCR and translation results.

**All CLI flags:**

| Flag                             | Description                   |
| -------------------------------- | ----------------------------- |
| `--debug-cli`                    | Headless mode, JSON to stdout |
| `--debug-cli --pretty`           | Pretty-printed output         |
| `--debug-cli --once`             | One OCR cycle then exit       |
| `--debug-cli --test-suite <dir>` | E2E test suite                |
| `--list-models`                  | Print manifest table          |
| `--prune-models`                 | Interactive cleanup           |

---

## 9. Hotkey & Controls

**Status:** ✅ Done.

| Hotkey            | Action                      |
| ----------------- | --------------------------- |
| `Cmd + Shift + T` | Toggle overlay visibility   |
| `Cmd + Shift + Q` | Quit application            |
| `Cmd + Shift + R` | Force OCR (bypass debounce) |
| `Cmd + Shift + M` | Clear translation memory    |
| `Cmd + Shift + G` | Toggle model tier           |

---

## 10. In-App Help

**Status:** ✅ Done — `help.html` bundled and accessible from tray.

---

## 11. Crash Reporting (Opt-in)

**Status:** ✅ Done — `sentry-rust` initialized only if user opted in during wizard Screen 4.

---

## 12. Silent Auto-Update

**Status:** ✅ Done — `tauri-plugin-updater` configured.

---

## 13. Memory Budget

| Component                    | Default (NLLB) | Quality (Gemma 4) | Native (Foundation) |
| ---------------------------- | -------------- | ----------------- | ------------------- |
| Translation model (resident) | ~1.2 GB        | ~5.0 GB           | ~0 GB               |
| llama-server sidecar process | ~50 MB         | ~50 MB            | N/A                 |
| Tauri WebView                | ~80 MB         | ~80 MB            | ~80 MB              |
| Rust backend + buffers       | ~50 MB         | ~50 MB            | ~50 MB              |
| Apple Vision (ANE)           | ~0 MB          | ~0 MB             | ~0 MB               |
| Frame buffers (2× 4K BGRA)   | ~96 MB         | ~96 MB            | ~96 MB              |
| **Total Estimated**          | **~1.5 GB**    | **~5.3 GB**       | **~0.3 GB**         |
| **Hard Ceiling**             | **3.0 GB**     | **8.0 GB**        | **1.0 GB**          |

---

## 14. Security & Privacy

- All processing is fully local — screen contents never leave the device
- Network requests: model download (Hugging Face), optional crash reports (Sentry, opt-in), update version checks only
- `llama-server` listens only on `127.0.0.1` — not accessible from network
- All network requests use HTTPS

---

## 15. Error Handling & Edge Cases

| Scenario                           | Behavior                                                                         |
| ---------------------------------- | -------------------------------------------------------------------------------- |
| `llama-server` fails to start      | Health check times out; show error dialog; app blocks translation until resolved |
| `llama-server` crashes mid-session | Watchdog restarts after 3 `/health` failures; non-blocking banner                |
| `vision-helper` binary missing     | Log error; emit `translation-error`; check app bundle integrity                  |
| `vision-helper` returns empty JSON | OCR returns empty array; overlay clears gracefully                               |
| No Japanese text on screen         | OCR empty array; overlay clears                                                  |
| Model file not found               | Wizard triggered; block app until model present                                  |
| Download interrupted               | Resume from `.part` file                                                         |
| SHA256 mismatch                    | Delete file; show retry dialog                                                   |
| Thermal throttling                 | Auto-degrade to NLLB; increase debounce; tray badge                              |
| Display unplugged                  | Stop SCStream; close overlay window                                              |
| Display added                      | Create new stream and overlay                                                    |
| Vertical Japanese text             | `text_angle` from vision-helper; `writing-mode: vertical-rl`                     |
| Furigana cluttering overlay        | Proximity suppression; configurable                                              |
| System RAM < 12GB                  | Quality Mode disabled; greyed tray item                                          |
| App switch                         | Context memory + overlay cleared                                                 |
| Browser tab switch                 | Context NOT cleared (same bundle ID)                                             |

---

## 16. Risk Register

| Risk                                               | Severity | Mitigation                                                                                    |
| -------------------------------------------------- | -------- | --------------------------------------------------------------------------------------------- |
| `screencapturekit` crate frame delivery broken     | High     | 1-day time-box; fallback to Swift capture helper via Unix socket                              |
| `vision-helper` compilation fails in CI            | Medium   | Compile during Tauri build; bundle in app Resources; test on clean machine                    |
| `llama-server` Metal init fails on first launch    | High     | Health check with 10s timeout + clear error dialog; fallback to CPU mode (`n_gpu_layers = 0`) |
| `llama-server` sidecar not codesigned              | High     | Sign with Developer ID; add to entitlements; test notarized build                             |
| Model file not present on first run                | High     | Wizard blocks until download complete; cannot reach overlay without model                     |
| `vision-helper` PNG temp file race condition       | Medium   | Write to unique temp path per frame; delete after subprocess exits                            |
| Sidecar HTTP port collision                        | Low      | Prefer `127.0.0.1:8765`; if busy, scan adjacent ports; log chosen port                        |
| Foundation Models guardrails reject mature content | Medium   | v1.1 concern; offer fallback to llama-server if FM returns empty                              |
| Translation engine crash                           | High     | `/health` watchdog poll every 5s; restart after 3 failures                                    |
| Multi-monitor overlay misalignment                 | Medium   | Per-display overlay windows; per-display `scale_factor`                                       |
| Retina coordinate mismatch                         | Medium   | `scale_factor` normalization; test on Retina + external monitor                               |
| Context memory poisons translations                | Medium   | Auto-clear on app switch; `Cmd+Shift+M`; 6-entry cap                                          |
| macOS notarization blocks `llama-server`           | High     | Must be codesigned and notarized as part of app bundle; plan this before Phase 8              |
