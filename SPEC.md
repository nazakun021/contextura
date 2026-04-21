# SPEC.md — Real-Time Screen Translation Overlay

**Version:** 1.2.0
**Target Platform:** macOS 13+ (Apple Silicon, M-series)
**Last Updated:** 2026-04-21

**Implementation Status:** Modules for Phases 0 through 7 (Hotkeys, Tray, Capture Types, Motion Debouncing, OCR Logic, Translation Memory, WebView structure, Double-buffering Performance, and E2E Testing scaffolding) are fully implemented and scaffolded in the `src-tauri` workspace.
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
|  |  | ScreenCaptureKit  |   |   |  Multiple Transparent      |  |
|  |  |  (per-display)    |   |   |  Overlay Windows           |  |
|  |  +--------+---------+   |   |  (one per display)         |  |
|  |           |              |   |  Absolutely-positioned     |  |
|  |  +--------v---------+   |   |  <div> translation boxes   |  |
|  |  |  Motion Delta     |   |   |  (horizontal & vertical)   |  |
|  |  |  Detector         |   |   +----------------------------+  |
|  |  +--------+---------+   |              ^                    |
|  |           |              |              | Tauri IPC          |
|  |  +--------v---------+   |              | (JSON payload)     |
|  |  |  Vision OCR       +---+--------------+                   |
|  |  |  (Apple ANE)      |   |                                   |
|  |  +--------+---------+   |                                   |
|  |           |              |                                   |
|  |  +--------v---------+   |                                   |
|  |  |  llama.cpp        |   |                                   |
|  |  |  NLLB / Gemma 4   |   |                                   |
|  |  |  (Metal GPU)      |   |                                   |
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

| Layer                       | Technology                       | Version Target     | Justification                                                |
| --------------------------- | -------------------------------- | ------------------ | ------------------------------------------------------------ |
| App Framework               | Tauri                            | v2.x               | Lightweight, native OS integration, IPC, transparent windows |
| Backend Language            | Rust                             | 1.78+ (stable)     | Memory safety, zero-cost abstractions, macOS FFI             |
| Screen Capture              | ScreenCaptureKit                 | macOS 13+          | Native, hardware-accelerated, low CPU overhead               |
| OCR                         | Apple Vision Framework           | via `objc2-vision` | ANE-accelerated, outputs bounding boxes + text angle         |
| Translation                 | llama.cpp                        | latest release     | Metal backend, GGUF model support, M-series optimized        |
| Translation Model (Default) | NLLB-200-distilled-600M (Q4_K_M) | —                  | ~1.2GB footprint, purpose-built for translation              |
| Translation Model (Quality) | Gemma 4 E4B IT (Q4_K_M)          | —                  | ~5GB, superior reasoning, 128K context window                |
| Parallelism                 | Rayon                            | latest             | CPU-bound tasks only (styling, serialization)                |
| Frontend                    | HTML5 / CSS3 / Vanilla JS        | —                  | Minimal, no framework overhead for overlay rendering         |
| Crash Reporting             | sentry-rust (opt-in)             | latest             | Anonymous crash telemetry, explicit user consent             |
| HTTP Client                 | reqwest                          | latest             | In-app model downloader with progress streaming              |

---

## 5. Subsystem Specifications

### 5.1 Screen Capture — ScreenCaptureKit

**Purpose:** Continuously capture the contents of each display into pixel buffers.

**Implementation Details:**

- On startup, enumerate all `NSScreen.screens()`
- For every active display, create one `SCStream` with its own `SCContentFilter`
- Use `SCStreamConfiguration`: `BGRA8Unorm` pixel format, 30 FPS (configurable via `settings.json`)
- Exclude each display's corresponding overlay window via `excludedWindows` (prevents capture loop)
- Deliver frames via a bounded `crossbeam` channel (capacity: 2, drop on full) per display

**Entitlements Required:**

```xml
<key>com.apple.security.screen-capture</key>
<true/>
```

**Multi-Monitor Behavior:**

- One Tauri overlay window per display, sized to that display's logical frame
- Each overlay is click-through (`set_ignore_cursor_events(true)`)
- Translation payloads carry a `display_id` so each event routes to the correct window

**Display Hot-Plug Handling:**

- Subscribe to `CGDisplayRegisterReconfigurationCallback`
- On display removed: stop its `SCStream`, drop its channel, close its Tauri window
- On display added: create new overlay window and capture stream, insert into manager

**Threading Model:**

- One frame-processing thread per display
- Threads share a single `TranslationEngine` instance behind `Arc<Mutex<>>`

---

### 5.2 Motion Detection (Delta Check)

**Purpose:** Gate expensive OCR/translation processing behind a scroll-stop trigger.

**Algorithm:**

```
1. Receive frame (CMSampleBuffer -> CVPixelBuffer)
2. Downscale to 160x90 grayscale thumbnail (bilinear interpolation)
3. Exclude a 5% inset margin from all four edges before comparison
   - Effective comparison area: ~152x81 pixels
   - Reason: Excludes dock animations, menu bar clock, scrollbar fades
4. Compute per-pixel absolute differences vs. previous thumbnail
5. Build a binary "changed pixel" mask: changed if diff > PIXEL_DIFF_THRESHOLD (default: 15)
6. Run a lightweight connected-components pass on the mask (4-connectivity union-find)
7. Find the largest single connected region of changed pixels
8. Compute that region's area as a fraction of total comparison area -> motion_ratio
9. Apply threshold: motion_ratio > 0.05 -> "motion detected"
```

**Why connected-components instead of raw pixel sum:**
Spinning loaders, blinking cursors, and looping ads produce scattered isolated changed pixels that sum to a high ratio but never form a large contiguous block. Scrolling always produces one large cohesive region. This distinction makes the debounce dramatically more reliable against UI noise.

**Debounce State Machine:**

```
States: SCROLLING | SETTLING | IDLE

SCROLLING:
  - motion > threshold  -> stay SCROLLING, reset timer to 300ms, hide overlay
  - motion <= threshold -> transition to SETTLING, start 300ms countdown

SETTLING:
  - motion > threshold  -> back to SCROLLING
  - timer hits 0        -> transition to IDLE, snap frame, trigger OCR pipeline

IDLE:
  - motion > threshold  -> back to SCROLLING, hide overlay
  - no new motion       -> stay IDLE (overlay remains visible)
```

**Constants (all configurable via `settings.json`):**

| Constant               | Default | Description                                           |
| ---------------------- | ------- | ----------------------------------------------------- |
| `MOTION_THRESHOLD`     | 0.05    | Fraction of largest contiguous changed-pixel region   |
| `PIXEL_DIFF_THRESHOLD` | 15      | Per-pixel absolute grayscale diff to count as changed |
| `DEBOUNCE_MS`          | 300     | Milliseconds of stillness before triggering OCR       |
| `EDGE_INSET_PERCENT`   | 5       | % of screen edge excluded from delta check            |
| `CAPTURE_FPS`          | 30      | Target frame capture rate                             |

---

### 5.3 OCR — Apple Vision Framework

**Purpose:** Extract Japanese text strings, bounding boxes, and text orientation from a static frame.

**Implementation:**

- Use `objc2-vision` crate with `VNRecognizeTextRequest`
- `recognitionLevel = .accurate`, `recognitionLanguages = ["ja-JP"]`, `usesLanguageCorrection = true`
- Extract `textAngle` (radians) per observation to detect vertical text

**Output per recognized region:**

```rust
struct OcrResult {
    text: String,
    confidence: f32,
    bounding_box: CGRect,  // normalized, bottom-left origin
    text_angle: f32,       // radians; |angle| > pi/4 -> vertical
    is_vertical: bool,
}
```

**Vertical Text Handling:**
When `is_vertical == true`:

- Swap `width` and `height` during coordinate conversion
- Pass `is_vertical: true` in the IPC payload
- Frontend applies `writing-mode: vertical-rl; text-orientation: mixed` to the overlay div

**Furigana Suppression:**
After OCR, post-process results:

1. Group bounding boxes by horizontal overlap (> 70%) with a larger box directly above/below
2. If a box's height is < 40% of the overlapping box's height, classify as furigana
3. Exclude from translation pipeline
4. Store as `furigana` field on parent box for future tooltip display (v1.1)
5. Configurable via `settings.json: furigana_suppression`

**Coordinate Conversion:**

- Vision uses bottom-left origin (normalized 0.0 to 1.0)
- Convert: `screen_y = (1.0 - vision_y - vision_height) * screen_height`
- Divide by `scale_factor` (per display) to get logical CSS points
- For vertical text: swap width/height after conversion

**Filtering:**

- Drop results with `confidence < 0.4`
- Drop results with no CJK characters (Unicode `\u{3040}` to `\u{9FFF}`)
- Merge overlapping boxes (IoU > 0.3)

---

### 5.4 Translation Engine — llama.cpp + NLLB / Gemma 4

**Purpose:** Translate extracted Japanese strings to English using a locally-running quantized model.

#### Model Tiers

| Tier             | Model                   | Size (Q4_K_M) | RAM    | Use Case                                   |
| ---------------- | ----------------------- | ------------- | ------ | ------------------------------------------ |
| **Default**      | NLLB-200-distilled-600M | ~1.2 GB       | Low    | Fast, purpose-built for translation        |
| **Quality Mode** | Gemma 4 E4B IT          | ~5 GB         | Higher | Nuanced Japanese, slang, narrative context |

**NLLB** is the default — a specialized machine translation model, fast and memory-efficient. **Gemma 4 E4B** is the Quality Mode tier (replaces previously planned ALMA). It has a 128K context window, superior Japanese reasoning, and fits the 16GB M2 memory budget. ALMA is removed from the plan entirely.

The active model is user-switchable via tray menu or `Cmd+Shift+G`, with a reload indicator shown during model swap.

**RAM Gate:** At startup, read total system RAM via `sysctl hw.memsize`. If total RAM < 12GB, disable Quality Mode entirely — grey out in tray with tooltip: "Gemma 4 requires at least 12GB of RAM."

**Integration:**

- Use `llama_cpp_rs` crate with Metal backend (`n_gpu_layers = 99`)
- Context size: 1024 tokens (accommodates context memory + full batch prompt)
- Load model once at startup; keep resident for the entire session

#### Translation Request Format — Batched Single-Pass with Rolling Context

```
[If context memory is non-empty:]
Previous context (do not retranslate, use for reference only):
- {memory[0].japanese} -> "{memory[0].english}"
- {memory[1].japanese} -> "{memory[1].english}"
...up to 6 entries

Translate each numbered Japanese string to English.
Output only the translations, one per line, in the same numbered format.

1: {ocr_result[0].text}
2: {ocr_result[1].text}
...N: {ocr_result[N].text}
```

**Output parsing:**

- Split response by newlines; match `^(\d+): (.+)$` per line
- Map back to original `OcrResult` by index
- Missing or malformed lines produce `""` (overlay shows original Japanese as fallback)
- If OCR returns > 15 strings: split into sequential sub-batches of 15

**Critical:** Do NOT use `rayon::par_iter()` for inference. Metal does not safely support concurrent KV cache allocation on the same model instance. Concurrent inference calls will cause memory contention and Metal driver crashes. Rayon is used only for CPU-bound work: color sampling, coordinate math, payload serialization.

#### Performance Targets

| Metric                                         | NLLB (Default) | Gemma 4 E4B (Quality) |
| ---------------------------------------------- | -------------- | --------------------- |
| Translation latency (single string, <50 chars) | < 800ms        | < 1.5s                |
| Translation latency (full screen, ~10 strings) | < 3s           | < 5s                  |
| Model load time at startup                     | < 5s           | < 8s                  |
| Peak RAM usage (model + app)                   | < 2GB          | < 6.5GB               |

#### Crash Recovery Watchdog

The translation engine runs in a supervised thread. A watchdog counts consecutive failures (timeout, Metal error, OOM). After 3 consecutive failures:

1. Restart the thread and reload the model
2. Emit `"translation-error"` to the frontend — non-blocking banner: "Translation engine restarted." (4s auto-dismiss)
3. Clear the overlay until the next successful translation

#### Thermal / Battery Awareness

Subscribe to IOKit power source and thermal notifications. When on battery AND thermal state is `serious` or `critical`:

- Force model to NLLB if Gemma 4 is active; send `ModelSwitch` invalidation
- Increase `DEBOUNCE_MS` to 600 in runtime state
- Show a thermal badge on the tray icon
- Restore normal behavior when plugged in or thermal state improves

---

### 5.5 Rolling Translation Memory

**Purpose:** Fix context blindness — the model's inability to resolve pronouns, dropped subjects, and narrative threads across sequential screens.

**State:**

```rust
struct TranslationMemory {
    entries: VecDeque<(String, String)>,  // (japanese_original, english_translation)
    max_size: usize,                       // default: 6, from settings.json
}
```

**State rules:**

- On `translation-clear` (scroll detected): keep memory — content is still related
- On active app change (Section 5.6): auto-clear — new app means new topic
- On `Cmd+Shift+M`: manual clear — user signals a topic break
- On model switch (`Cmd+Shift+G`): auto-clear — context from different model is unreliable
- On app restart: memory does not persist (session-scoped only)

**Tradeoffs:**

| Risk              | Description                                          | Mitigation                                    |
| ----------------- | ---------------------------------------------------- | --------------------------------------------- |
| Context poisoning | A wrong translation compounds into subsequent ones   | Auto-clear on app switch; manual clear hotkey |
| Topic bleed       | Tab switch carries irrelevant context to new content | App-change detection auto-clears              |
| Stale noise       | Old entries become irrelevant filler                 | 6-entry cap keeps window fresh                |
| Prompt overhead   | Slightly longer prompts = minor inference slowdown   | ~150 tokens; negligible on Gemma 4            |

---

### 5.6 Context Invalidation Strategy

**Purpose:** Prevent translation context from bleeding across unrelated content when the user switches applications or manually signals a topic change.

**Detection Mechanism:** `NSWorkspaceDidActivateApplicationNotification`

Note: ScreenCaptureKit captures the full display as a pixel stream and does NOT reliably expose per-frame frontmost-app metadata. The correct macOS mechanism for detecting the active application is `NSWorkspaceDidActivateApplicationNotification`.

```rust
struct AppWindowTracker {
    current_bundle_id: Option<String>,
    invalidation_tx: Sender<InvalidationReason>,
}

enum InvalidationReason {
    AppSwitch { from: String, to: String },
    ManualReset,
    ModelSwitch,
}
```

**Notification flow:**

```
NSWorkspaceDidActivateApplicationNotification fires
  -> read new app's bundleIdentifier
  -> compare to stored current_bundle_id
  -> if different:
      - update current_bundle_id
      - send InvalidationReason::AppSwitch on channel
      - TranslationMemory::clear()
      - emit "translation-clear" to Tauri frontend
      - log: "[ContextInvalidation] AppSwitch: Safari -> iTerm2 - memory cleared"
```

**Invalidation Trigger Matrix:**

| Trigger       | Source                                          | Clears Memory      | Clears Overlay |
| ------------- | ----------------------------------------------- | ------------------ | -------------- |
| App switch    | `NSWorkspaceDidActivateApplicationNotification` | Yes                | Yes            |
| Manual reset  | `Cmd+Shift+M` hotkey                            | Yes                | No             |
| Model switch  | `Cmd+Shift+G` hotkey                            | Yes                | Yes            |
| App restart   | Process init                                    | Yes (never loaded) | N/A            |
| Scroll/motion | Debounce state machine                          | No                 | Yes            |

**Note on overlay vs. memory:** These are independent operations. A manual reset keeps the overlay visible (user may still be reading). An app switch clears both because the screen content has fundamentally changed.

**What does NOT trigger invalidation:**

- Browser tab switches within the same app (Safari stays Safari — multi-tab reading context is intentionally preserved)
- Scrolling within a page

**Fallback:** If notification subscription fails, poll `NSWorkspace.shared.frontmostApplication` every 2 seconds in a background thread. Log: "[AppTracker] Notification subscription failed, using 2s polling fallback".

---

### 5.7 Dynamic Styling

**Purpose:** Ensure translated text is always readable against any background color.

**Algorithm per bounding box:**

```
1. Sample the outer 2px border of the bounding box from the pixel buffer
2. Average the RGBA values -> bg_color: (r, g, b)
3. Calculate relative luminance (WCAG 2.1 formula):
   L = 0.2126 * linearize(r) + 0.7152 * linearize(g) + 0.0722 * linearize(b)
   where linearize(c) = c/12.92 if c <= 0.04045 else ((c+0.055)/1.055)^2.4
4. L > 0.179 -> fg_color = "#000000" (dark text on light bg)
   L <= 0.179 -> fg_color = "#FFFFFF" (light text on dark bg)
5. overlay_bg = bg_color at 85% opacity -> "rgba(r, g, b, 0.85)"
```

---

### 5.8 IPC Payload — Rust to Tauri Frontend

```typescript
interface TranslationBox {
  id: string;
  translated: string;
  original: string; // Japanese original (tooltip, fallback display)
  x: number;
  y: number;
  width: number;
  height: number;
  is_vertical: boolean; // drives CSS writing-mode
  bg_color: string; // e.g. "rgba(30, 30, 30, 0.85)"
  fg_color: string; // "#000000" or "#FFFFFF"
  confidence: number;
}

type TranslationPayload = {
  boxes: TranslationBox[];
  scale_factor: number;
  display_id: number; // routes payload to correct overlay window
  frame_id: number;
};
```

**Events emitted by Rust backend:**

| Event                   | Payload              | Trigger                         |
| ----------------------- | -------------------- | ------------------------------- |
| `"translation-update"`  | `TranslationPayload` | New translation batch ready     |
| `"translation-clear"`   | none                 | Motion detected or app switched |
| `"translation-started"` | `{ display_id }`     | Inference batch has begun       |
| `"translation-error"`   | `{ message }`        | Watchdog restarted engine       |

---

### 5.9 Tauri Overlay Window

**Per-display window configuration:**

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

**Rust setup per window:**

```rust
window.set_ignore_cursor_events(true)?;   // click-through
window.set_content_protection(false)?;    // allow SCKit capture (no feedback loop)
```

**Frontend Rendering:**

- `"translation-update"` — clear existing `.translation-box` elements; render new absolutely-positioned divs
- `"translation-clear"` — remove all boxes immediately
- `"translation-started"` — show subtle bottom-right spinner: "Translating..." (opacity 0.6, pointer-events none)
- `"translation-error"` — show non-blocking top banner, auto-dismiss after 4s
- Vertical boxes: apply `writing-mode: vertical-rl; text-orientation: mixed`
- All boxes: CSS transition `opacity 0.15s ease-in` on appearance

---

## 6. Model Download & First-Run Onboarding

### 6.1 Onboarding Wizard (4 Screens)

**Screen 1 — Screen Recording Permission:**

- Explains why screen recording is needed in plain language
- Button opens System Settings -> Privacy -> Screen Recording
- Polls for permission grant before allowing progress to Screen 2

**Screen 2 — Model Selection:**

- Standard (NLLB ~1.2GB): "Fast, great for course materials and everyday reading"
- Quality (Gemma 4 ~5GB): "Handles nuanced Japanese better. Requires 12GB+ RAM."
- Quality option is greyed out with explanation if system RAM < 12GB

**Screen 3 — Download & Verification:**

- Progress bar showing percentage and MB/s transfer speed
- Uses HTTP Range requests + `.part` sidecar file for resumable downloads
- Renames `.part` to `.gguf` only after successful SHA256 verification
- On SHA256 mismatch: delete corrupt file, show dialog: "Verification failed. Retry download?"
- If wizard is closed mid-download: download continues in background; tray shows "Downloading model (45%)" with cancel option
- Post-download: 30-second interactive demo on a bundled sample Japanese image

**Screen 4 — Privacy:**

- Lists all three categories of network requests this app ever makes:
  1. Model download from Hugging Face (setup only)
  2. Optional anonymous crash reports via Sentry (opt-in, off by default)
  3. Silent update version checks (version number only, on startup)
- Opt-in Sentry checkbox with link to privacy policy (hosted on project GitHub)
- "The app never sends screen contents anywhere."

### 6.2 Model Management

**Storage path:** `~/Library/Application Support/jp-translate/models/`

**Manifest file:** `models/manifest.json`

```json
{
  "models": [
    {
      "id": "nllb-600m-q4",
      "filename": "nllb-200-distilled-600M.Q4_K_M.gguf",
      "size_bytes": 1288490188,
      "sha256": "<hash>",
      "downloaded_at": "2026-04-18T00:00:00Z",
      "last_used_at": "2026-04-18T00:00:00Z",
      "active": true
    },
    {
      "id": "gemma4-e4b-q4",
      "filename": "gemma-4-e4b-it.Q4_K_M.gguf",
      "size_bytes": 5368709120,
      "sha256": "<hash>",
      "downloaded_at": "2026-04-18T00:00:00Z",
      "last_used_at": "2026-04-18T00:00:00Z",
      "active": false
    }
  ]
}
```

**Management Rules:**

- On startup: scan for orphan `.gguf` files not in manifest — offer deletion
- On startup: non-active model with `last_used_at` > 30 days — prompt to prune
- Never silently delete — always require explicit user confirmation
- Hard warning at 4GB total model directory size; block new downloads until cleaned up
- `--list-models` flag: print manifest table to stdout
- `--prune-models` flag: interactive model cleanup wizard

---

## 7. Settings File

**Path:** `~/Library/Application Support/jp-translate/settings.json`

Created with defaults on first launch if missing. Changes take effect on app restart. Full settings UI is deferred to v1.1. Users access via tray menu -> "Open Settings File" which reveals it in Finder.

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

**Activation:** `cargo run -- --debug-cli`

Bypasses Tauri window creation entirely. Runs the full Rust engine (capture -> motion -> OCR -> translation -> styling) and prints JSON to stdout on each debounce trigger.

**Example output:**

```json
{
  "frame_id": 42,
  "trigger_latency_ms": 287,
  "ocr_duration_ms": 145,
  "translation_duration_ms": 834,
  "boxes": [
    {
      "id": "42-0",
      "original": "日本語のテキスト",
      "translated": "Japanese text",
      "x": 320.0,
      "y": 180.0,
      "width": 200.0,
      "height": 24.0,
      "is_vertical": false,
      "bg_color": "rgba(255,255,255,0.85)",
      "fg_color": "#000000",
      "confidence": 0.97
    }
  ]
}
```

**All CLI flags:**

| Flag                             | Description                                                  |
| -------------------------------- | ------------------------------------------------------------ |
| `--debug-cli`                    | Headless mode, JSON to stdout                                |
| `--debug-cli --pretty`           | Pretty-printed JSON output                                   |
| `--debug-cli --once`             | Trigger exactly one OCR cycle then exit                      |
| `--debug-cli --test-suite <dir>` | Run E2E test suite against directory of PNGs + expected JSON |
| `--list-models`                  | Print manifest table and exit                                |
| `--prune-models`                 | Interactive model cleanup wizard                             |

**`--test-suite <dir>` mode:**

- Directory contains pre-captured Japanese PNG screenshots
- Each PNG has a companion `.expected.json`:
  ```json
  {
    "ocr_must_contain": ["日本語", "テキスト"],
    "translation_must_contain": ["Japanese", "text"]
  }
  ```
- Runs full pipeline (OCR + translation) per image
- Asserts OCR substrings present; translation matches within similarity threshold
- Prints pass/fail per image; exits `0` (all pass) or `1` (any fail)
- Designed to run in GitHub Actions CI on every commit

---

## 9. Hotkey & Controls

| Hotkey            | Action                                            |
| ----------------- | ------------------------------------------------- |
| `Cmd + Shift + T` | Toggle overlay visibility on/off                  |
| `Cmd + Shift + Q` | Quit application                                  |
| `Cmd + Shift + R` | Force OCR on current screen (bypass debounce)     |
| `Cmd + Shift + M` | Manually clear translation memory (context reset) |
| `Cmd + Shift + G` | Toggle between NLLB and Gemma 4 E4B quality mode  |

All registered as global shortcuts via Tauri's `globalShortcut` plugin — functional even when the overlay is not focused.

---

## 10. In-App Help

Accessible via tray menu -> "Help". Opens `help.html`, a page bundled inside the `.app`:

- How to grant screen recording permission
- All 5 hotkeys with descriptions
- How to switch between NLLB and Gemma 4; what Quality Mode requires
- What context memory is, when it clears, how to manually reset it
- FAQ:
  - "Why isn't the overlay appearing?" — Check the app is running, screen permission is granted, and you've fully stopped scrolling for 300ms
  - "Translations look wrong or incomplete" — Try Quality Mode (`Cmd+Shift+G`)
  - "App feels slow or the Mac is getting warm" — Check the thermal badge in the menu bar; close other apps if using Quality Mode
  - "How do I delete a model to free up space?" — Tray -> Manage Models

---

## 11. Crash Reporting (Opt-in)

- Integrated via `sentry-rust`
- Opt-in prompt on Screen 4 of onboarding wizard; off by default
- Only anonymous stack traces and app version transmitted — no screen contents, no file paths
- User can change preference via tray -> "Settings -> Privacy"

---

## 12. Silent Auto-Update

- Integrated via `tauri-plugin-updater`
- Checks a public GitHub Releases JSON feed silently on startup
- If a new version is found: non-intrusive tray notification: "A new version is available. Restart to update."
- Never forces updates; user always opts in

---

## 13. Memory Budget

| Component                  | Default (NLLB) | Quality Mode (Gemma 4) |
| -------------------------- | -------------- | ---------------------- |
| Translation model          | ~1.2 GB        | ~5.0 GB                |
| Tauri WebView              | ~80 MB         | ~80 MB                 |
| Rust backend + buffers     | ~50 MB         | ~50 MB                 |
| Apple Vision (ANE)         | ~0 MB          | ~0 MB                  |
| Frame buffers (2x 4K BGRA) | ~96 MB         | ~96 MB                 |
| **Total Estimated**        | **~1.5 GB**    | **~5.3 GB**            |
| **Hard Ceiling**           | **3.0 GB**     | **8.0 GB**             |

Quality Mode on a 16GB system: close memory-heavy apps before switching. Quality Mode is fully disabled if system RAM < 12GB total.

---

## 14. Security & Privacy

- All processing is fully local — screen contents never leave the device
- Network requests limited to three categories: model download (Hugging Face), optional crash reports (Sentry, opt-in), silent update version checks
- Screen capture usage disclosed on Screen 1 of onboarding wizard
- Privacy policy hosted on project GitHub, linked from Screen 4 of onboarding wizard
- All network requests use HTTPS

---

## 15. Error Handling & Edge Cases

| Scenario                      | Behavior                                                                 |
| ----------------------------- | ------------------------------------------------------------------------ |
| No Japanese text on screen    | OCR returns empty array; overlay clears                                  |
| Screen permission denied      | Onboarding wizard blocks progress; shows system settings link            |
| Model file not found          | First-run wizard triggered; blocks app until model is present            |
| Download interrupted          | Resumes from last byte via `.part` file on next launch                   |
| SHA256 mismatch               | Delete corrupt file; show retry dialog                                   |
| Translation takes > 5s        | Show "Translating..." spinner; timeout after 10s, show original Japanese |
| Translation engine crash      | Watchdog restarts after 3 failures; non-blocking banner shown            |
| Thermal throttling            | Auto-degrades to NLLB + 600ms debounce; tray badge shown                 |
| Display unplugged             | Stop SCStream, close overlay window, clean up manager state              |
| Display added                 | Create new SCStream and overlay window                                   |
| App is capturing itself       | `excludedWindows` prevents feedback loop at init                         |
| User switches application     | Context memory cleared; overlay cleared                                  |
| Browser tab switch            | Context memory NOT cleared (same bundle ID — intentional)                |
| Resolution change mid-session | Re-query `scale_factor` per display; update stream config                |
| Vertical Japanese text        | `textAngle` detected; overlay div uses `writing-mode: vertical-rl`       |
| Furigana cluttering overlay   | Suppressed by proximity post-processing; configurable in settings        |
| System RAM < 12GB             | Quality Mode disabled at startup; user notified via greyed tray item     |

---

## 16. Risk Register

| Risk                                           | Severity | Mitigation                                                                      |
| ---------------------------------------------- | -------- | ------------------------------------------------------------------------------- |
| `objc2-vision` bindings incomplete or broken   | Low      | Crate is production-ready as of v0.2+; Swift subprocess bridge as fallback      |
| Vertical text rendering fails                  | Medium   | Use `textAngle` from Vision; CSS `writing-mode`; test against manga screenshots |
| Furigana suppression over-eager                | Medium   | Conservative thresholds (< 40% height, > 70% overlap); toggle in settings.json  |
| Translation engine crash                       | High     | Supervised thread watchdog; auto-restart after 3 consecutive failures           |
| Multi-monitor overlay misalignment             | Medium   | Per-display overlay windows with independent coordinate mapping                 |
| Thermal throttling degrades UX silently        | Medium   | IOKit monitoring auto-degrades to NLLB; user-visible tray badge                 |
| Non-technical user cannot install              | High     | In-app onboarding wizard with guided download eliminates manual file copy       |
| No crash visibility post-release               | Medium   | Opt-in Sentry with explicit consent; anonymous stack traces only                |
| Download interrupted during setup              | Medium   | Resumable downloads with `.part` sidecar file                                   |
| Quality Mode causes RAM pressure               | Medium   | 12GB RAM gate at startup; warn before switching if free RAM < 8GB               |
| Retina coordinate mismatch                     | Medium   | Per-display `scale_factor` normalization from Phase 1                           |
| macOS notarization requirements                | Low      | Tauri built-in signing toolchain; Apple Developer account required              |
| Context memory poisons subsequent translations | Medium   | Auto-clear on app switch; `Cmd+Shift+M` manual clear; 6-entry cap               |
