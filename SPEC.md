# SPEC.md — Real-Time Screen Translation Overlay

**Version:** 1.0.0  
**Target Platform:** macOS 13+ (Apple Silicon, M-series)  
**Last Updated:** 2026-04-18

---

## 1. Project Overview

A high-performance desktop overlay application that detects Japanese text on-screen, translates it to English in real-time using local AI models, and renders translated text as a transparent, click-through overlay precisely positioned over the original text. The application runs entirely offline after initial model download.

---

## 2. Goals & Non-Goals

### Goals

- Translate Japanese text visible in any macOS application (browsers, PDFs, video players, etc.)
- Maintain real-time responsiveness with sub-2-second end-to-end latency after the debounce trigger
- Keep total RAM footprint under 4GB to prevent SSD swapping on 16GB unified memory systems
- Operate without any internet connection after setup
- Be thermally responsible — no continuous GPU/ANE hammering

### Non-Goals

- Translating audio or video subtitles in real-time (frame-by-frame)
- Supporting languages other than Japanese → English in v1.0
- Providing a history, clipboard, or dictionary lookup feature in v1.0
- Running on Intel Macs or non-Apple hardware

---

## 3. System Architecture

```
┌─────────────────────────────────────────────────────────┐
│                    TAURI APPLICATION                     │
│                                                          │
│  ┌─────────────────────┐    ┌────────────────────────┐  │
│  │   Rust Backend       │    │   WebView Frontend     │  │
│  │                      │    │   (HTML/CSS/JS)        │  │
│  │  ┌────────────────┐  │    │                        │  │
│  │  │ScreenCaptureKit│  │    │  Transparent Overlay   │  │
│  │  └───────┬────────┘  │    │  Absolutely-positioned │  │
│  │          │            │    │  <div> translation     │  │
│  │  ┌───────▼────────┐  │    │  boxes                 │  │
│  │  │  Motion Delta  │  │    │                        │  │
│  │  │  Detector      │  │    └────────────────────────┘  │
│  │  └───────┬────────┘  │              ▲                  │
│  │          │            │              │ Tauri IPC        │
│  │  ┌───────▼────────┐  │              │ (JSON payload)   │
│  │  │  Vision OCR    │  │──────────────┘                  │
│  │  │  (Apple ANE)   │  │                                  │
│  │  └───────┬────────┘  │                                  │
│  │          │            │                                  │
│  │  ┌───────▼────────┐  │                                  │
│  │  │  llama.cpp     │  │                                  │
│  │  │  NLLB Model    │  │                                  │
│  │  │  (Metal GPU)   │  │                                  │
│  │  └────────────────┘  │                                  │
│  └─────────────────────┘                                  │
└─────────────────────────────────────────────────────────┘
```

---

## 4. Technology Stack

| Layer             | Technology                       | Version Target       | Justification                                                |
| ----------------- | -------------------------------- | -------------------- | ------------------------------------------------------------ |
| App Framework     | Tauri                            | v2.x                 | Lightweight, native OS integration, IPC, transparent windows |
| Backend Language  | Rust                             | 1.78+ (stable)       | Memory safety, zero-cost abstractions, macOS FFI             |
| Screen Capture    | ScreenCaptureKit                 | macOS 13+            | Native, hardware-accelerated, low CPU overhead               |
| OCR               | Apple Vision Framework           | via `objc2` bindings | ANE-accelerated, outputs bounding boxes, no RAM overhead     |
| Translation       | llama.cpp                        | latest release       | Metal backend, GGUF model support, M-series optimized        |
| Translation Model | NLLB-200-distilled-600M (Q4_K_M) | —                    | ~1.2GB footprint, production Japanese→English quality        |
| Parallelism       | Rayon                            | latest               | Data-parallel OCR batch translation                          |
| Frontend          | HTML5 / CSS3 / Vanilla JS        | —                    | Minimal, no framework overhead for overlay rendering         |

---

## 5. Subsystem Specifications

### 5.1 Screen Capture — ScreenCaptureKit

**Purpose:** Continuously capture the display contents into pixel buffers.

**Implementation Details:**

- Use `SCStreamConfiguration` to request 30 FPS capture (configurable)
- Request pixel format: `BGRA8Unorm` for direct buffer math
- Capture scope: Full display (configurable to window-only in future)
- Deliver frames to a Rust callback via `SCStreamOutput` delegate

**Entitlements Required:**

```xml
<key>com.apple.security.screen-capture</key>
<true/>
```

The app must also be added to System Preferences → Privacy & Security → Screen Recording on first launch.

**Threading Model:**

- Frame callbacks arrive on a dedicated ScreenCaptureKit dispatch queue
- Frames are passed via a bounded `crossbeam` channel (capacity: 2) to the motion detector thread
- Frames that arrive while the channel is full are **dropped** (backpressure)

---

### 5.2 Motion Detection (Delta Check)

**Purpose:** Gate expensive OCR/translation processing behind a scroll-stop trigger.

**Algorithm:**

```
1. Receive frame (CMSampleBuffer → CVPixelBuffer)
2. Downscale to 160×90 grayscale thumbnail (bilinear interpolation)
3. Exclude a 5% inset margin from all four edges before comparison
   - Effective comparison area: ~152×81 pixels
   - Reason: Excludes dock animations, menu bar clock, scrollbar fades
4. Compute per-pixel absolute differences vs. previous thumbnail
5. Build a binary "changed pixel" mask: changed if diff > PIXEL_DIFF_THRESHOLD (default: 15)
6. Run a lightweight connected-components pass on the mask (4-connectivity flood fill)
7. Find the largest single connected region of changed pixels
8. Compute that region's area as a fraction of total comparison area → motion_ratio
9. Apply threshold: motion_ratio > 0.05 → "motion detected"
```

**Why connected-components instead of raw pixel count:**
Raw pixel-diffing is susceptible to localized noise sources — spinning loading spinners, blinking cursors, looping ad animations, and video thumbnails. These produce scattered, isolated changed pixels that sum to a high `motion_ratio` even though no meaningful scroll has occurred. By requiring the changed pixels to form a _single large contiguous block_, the algorithm correctly distinguishes scroll (large cohesive region moving uniformly) from noise (dozens of tiny isolated regions).

**Performance note:** The connected-components pass operates on a 152×81 image (~12,000 pixels). A single-pass union-find implementation completes in microseconds and does not materially affect the 30 FPS pipeline.

**Debounce State Machine:**

```
States: SCROLLING | SETTLING | IDLE

SCROLLING:
  - motion > 5%  → stay SCROLLING, reset timer to 300ms, hide overlay
  - motion < 5%  → transition to SETTLING, start 300ms countdown

SETTLING:
  - motion > 5%  → transition back to SCROLLING
  - timer hits 0 → transition to IDLE, snap frame, trigger OCR pipeline

IDLE:
  - new frame with motion > 5% → transition to SCROLLING, hide overlay
  - no new motion → stay IDLE (overlay remains visible)
```

**Constants (configurable via settings):**
| Constant | Default | Description |
|---|---|---|
| `MOTION_THRESHOLD` | 0.05 | Fraction of the largest contiguous changed-pixel region to count as motion |
| `PIXEL_DIFF_THRESHOLD` | 15 | Per-pixel absolute grayscale difference to count a pixel as "changed" |
| `DEBOUNCE_MS` | 300 | Milliseconds of stillness before triggering OCR |
| `EDGE_INSET_PERCENT` | 5 | % of screen edge to ignore in delta check |
| `CAPTURE_FPS` | 30 | Target frame capture rate |

---

### 5.3 OCR — Apple Vision Framework

**Purpose:** Extract Japanese text strings and their on-screen bounding boxes.

**Implementation Details:**

- Use `VNRecognizeTextRequest` with `recognitionLevel = .accurate`
- Set `recognitionLanguages = ["ja-JP"]`
- Access via `objc2` unsafe bindings (see Risk Register)
- Input: `CVPixelBuffer` from the static snapshot frame
- Output per recognized region:
  ```rust
  struct OcrResult {
      text: String,           // Recognized Japanese string
      confidence: f32,        // 0.0 – 1.0
      bounding_box: Rect,     // In normalized coordinates (0.0–1.0)
  }
  ```

**Coordinate System Note:**

- Vision returns normalized coordinates in a **bottom-left origin** system
- Must be converted to top-left origin before sending to Tauri
- Formula: `screen_y = (1.0 - vision_y - vision_height) * screen_height`

**HiDPI / Retina Scaling:**

- Vision bounding boxes are in _logical points_, not physical pixels
- The screen's `scale_factor` (typically `2.0` on Retina) must be tracked
- All coordinate math operates in logical points; `scale_factor` is passed to the frontend for CSS rendering

**Filtering:**

- Drop results with `confidence < 0.4`
- Drop results where `text` contains no CJK characters (Unicode range `\u{3000}–\u{9FFF}`)
- Merge overlapping bounding boxes (IoU > 0.3) into single regions

---

### 5.4 Translation Engine — llama.cpp + NLLB / Gemma 4

**Purpose:** Translate extracted Japanese strings to English using a locally-running quantized model.

#### Model Tiers

| Tier             | Model                   | Size (Q4_K_M) | RAM    | Use Case                                        |
| ---------------- | ----------------------- | ------------- | ------ | ----------------------------------------------- |
| **Default**      | NLLB-200-distilled-600M | ~1.2 GB       | Low    | Fast, reliable, purpose-built for translation   |
| **Quality Mode** | Gemma 4 E4B (IT)        | ~5 GB         | Higher | Nuanced Japanese, slang, long narrative context |

**NLLB** is the default because it's a specialized machine translation model — fast, memory-efficient, and accurate for straightforward course material Japanese. **Gemma 4 E4B** is the Quality Mode replacement for ALMA (previously planned). It fits the 16GB unified memory budget, has dramatically better Japanese reasoning, and its 128K context window makes the rolling translation memory essentially free to use. ALMA has been removed from the plan entirely.

The active model is user-switchable via the menu bar icon or `Cmd+Shift+G` hotkey, with a brief reload indicator (~3–5s) shown during model swap.

---

#### Rolling Translation Memory (Context Window)

**Purpose:** Fix context blindness — the model's inability to resolve pronouns, dropped subjects, and running narrative threads across sequential screens.

**How it works:**
The app maintains an in-memory `Vec<(String, String)>` of `(japanese_original, english_translation)` pairs from previous screens. This is prepended to every batch prompt as read-only context:

```
Previous context (do not retranslate):
- 先生は田中さんに言いました → "The teacher said to Tanaka-san:"
- 明日までに終わらせてください → "Please finish this by tomorrow."

Now translate these new strings:
1: わかりました
2: 頑張ります
```

**State rules:**

- Window size: last **6 translation pairs** (configurable; ~150 tokens overhead)
- On `translation-clear` (scroll detected): **keep memory** — content is still related
- On **active application change** (see Section 5.5 below): **auto-clear memory** — new app = new topic
- On `Cmd+Shift+M` hotkey: **manual clear** — user explicitly signals a context break
- On app restart: memory does not persist (session-scoped only)

**Tradeoffs to be aware of:**

| Risk              | Description                                              | Mitigation                                    |
| ----------------- | -------------------------------------------------------- | --------------------------------------------- |
| Context poisoning | A wrong translation feeds errors into subsequent ones    | Auto-clear on app switch; manual clear hotkey |
| Topic bleed       | Switching tabs carries irrelevant context to new content | App-change detection auto-clears              |
| Stale noise       | Very old entries become irrelevant filler                | 6-entry cap keeps window fresh                |
| Prompt length     | Slightly longer prompts = minor inference slowdown       | Negligible on Gemma 4; small on NLLB          |

---

### 5.5 Context Invalidation Strategy

**Purpose:** Prevent translation context from bleeding across unrelated content when the user switches applications or manually signals a topic change.

#### Detection Mechanism — `NSWorkspace` App Change Notifications

> **Important correction from earlier spec drafts:** ScreenCaptureKit captures the full display as a pixel stream and does **not** reliably expose per-frame frontmost-app metadata. The correct and idiomatic macOS mechanism for detecting the active application is `NSWorkspaceDidActivateApplicationNotification`.

**Implementation:**

```rust
// In Rust via objc2-app-kit / objc2-foundation:
// Subscribe to NSWorkspaceDidActivateApplicationNotification on startup.
// The notification delivers an NSRunningApplication object with:
//   - .bundleIdentifier  → e.g. "com.apple.Safari"
//   - .localizedName     → e.g. "Safari"
//   - .processIdentifier → PID

struct AppWindowTracker {
    current_bundle_id: Option<String>,
    // Sender half of a channel to the TranslationMemory owner
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
  → callback reads new app's bundleIdentifier
  → compare to stored current_bundle_id
  → if different:
      - update current_bundle_id
      - send InvalidationReason::AppSwitch on channel
      - TranslationMemory receives message → calls clear()
      - Emit "translation-clear" to Tauri frontend (hide overlay boxes)
      - Log: "[ContextInvalidation] App switched from Safari to iTerm2 — memory cleared"
```

#### Invalidation Trigger Matrix

| Trigger       | Source                                          | Clears Memory     | Clears Overlay | Log Message                   |
| ------------- | ----------------------------------------------- | ----------------- | -------------- | ----------------------------- |
| App switch    | `NSWorkspaceDidActivateApplicationNotification` | ✅                | ✅             | `AppSwitch: {from} → {to}`    |
| Manual reset  | `Cmd+Shift+M` hotkey                            | ✅                | ❌             | `ManualReset: user-initiated` |
| Model switch  | `Cmd+Shift+G` hotkey                            | ✅                | ✅             | `ModelSwitch: {old} → {new}`  |
| App restart   | Process init                                    | ✅ (never loaded) | N/A            | N/A                           |
| Scroll/motion | Debounce state machine                          | ❌                | ✅             | N/A                           |

**Note on overlay vs. memory:** Clearing the overlay (hiding translation boxes) and clearing the memory (wiping context history) are independent operations. A manual memory reset does **not** hide current translation boxes — the user may still be reading them. An app switch clears both because the screen content has fundamentally changed.

#### What Does NOT Trigger Invalidation

- Switching browser **tabs within the same app** — Safari stays Safari; context is intentionally preserved so multi-tab Japanese reading sessions stay coherent
- Scrolling within a page — the debounce handles the overlay; memory persists
- The overlay window itself gaining/losing focus (it's click-through, so this shouldn't occur)

#### Graceful Degradation

If `NSWorkspaceDidActivateApplicationNotification` cannot be subscribed (e.g., sandbox restriction), fall back to polling `NSWorkspace.shared.frontmostApplication` every 2 seconds in a background thread. Log a warning at startup: `"[AppTracker] Notification subscription failed, using 2s polling fallback"`.

---

### 3.1 Model Download & Storage

**Storage path:** `~/Library/Application Support/jp-translate/models/`

**Model manifest file:** `models/manifest.json` — tracks each downloaded model's filename, size, hash, download date, and last-used date.

```json
{
  "models": [
    {
      "id": "nllb-600m-q4",
      "filename": "nllb-200-distilled-600M.Q4_K_M.gguf",
      "size_bytes": 1288490188,
      "sha256": "...",
      "downloaded_at": "2026-04-18T00:00:00Z",
      "last_used_at": "2026-04-18T00:00:00Z",
      "active": true
    },
    {
      "id": "gemma4-e4b-q4",
      "filename": "gemma-4-e4b-it.Q4_K_M.gguf",
      "size_bytes": 5368709120,
      "sha256": "...",
      "downloaded_at": "2026-04-18T00:00:00Z",
      "last_used_at": "2026-04-18T00:00:00Z",
      "active": false
    }
  ]
}
```

**Model Management Rules:**

- On startup, scan `models/` for `.gguf` files not referenced in `manifest.json` (orphans) and offer to delete them
- On startup, if a model's `last_used_at` is more than 30 days ago and it is not the active model, prompt the user to delete it
- Never silently delete model files — always require explicit user confirmation (dialog or CLI `--prune-models` flag)
- Enforce a hard storage warning at 4GB total in the models directory; block new downloads and show a cleanup prompt
- Expose a `--list-models` CLI flag that prints the manifest table to stdout

**Rationale:** Between NLLB (~1.2GB), Gemma 4 E4B (~5GB), Xcode toolchain, and Rust crate cache, a 256GB drive fills up faster than expected. Proactive model management prevents silent disk exhaustion.

**Translation Request Format — Batched Single-Pass with Rolling Context:**

All OCR results from a single frame are batched into one structured prompt with optional preceding context from recent screens:

```
[If context memory is non-empty]
Previous context (do not retranslate, use for reference only):
- {memory[0].japanese} → "{memory[0].english}"
- {memory[1].japanese} → "{memory[1].english}"
...up to 6 entries

Translate each numbered Japanese string to English.
Output only the translations, one per line, in the same numbered format.

1: {ocr_result[0].text}
2: {ocr_result[1].text}
...N: {ocr_result[N].text}
```

**Expected model output:**

```
1: {english_translation_0}
2: {english_translation_1}
...
```

**Output parsing:**

- Split response by newlines
- Match `^(\d+): (.+)$` per line
- Map index back to original `OcrResult` by position
- If a line is missing or malformed, mark that box's translation as `""` (overlay shows original Japanese as fallback)

**Why not `rayon::par_iter()` for inference:**
Metal's memory model does not safely support concurrent inference contexts on the same model instance. Two threads simultaneously allocating KV cache on the same GPU backend is undefined behavior in llama.cpp's Metal path. Rayon parallelism is still used for CPU-bound work (color sampling, coordinate math, payload serialization) — just not for inference itself.

**Batch size cap:** If OCR returns more than 15 strings, split into sequential sub-batches of 15 to stay within the model's context window.

**Performance Targets:**
| Metric | NLLB (Default) | Gemma 4 E4B (Quality) |
|---|---|---|
| Translation latency (single string, <50 chars) | < 800ms | < 1.5s |
| Translation latency (full screen, ~10 strings) | < 3s | < 5s |
| Model load time at startup | < 5s | < 8s |
| Peak RAM usage (model + app) | < 2GB | < 6.5GB |

---

### 5.5 Dynamic Styling

**Purpose:** Ensure translated text is always readable against any background color.

**Algorithm per bounding box:**

```
1. Sample the outer 2px border of the bounding box from the pixel buffer
2. Average the RGBA values → bg_color: (r, g, b)
3. Calculate relative luminance (WCAG 2.1 formula):
   L = 0.2126 * linearize(r) + 0.7152 * linearize(g) + 0.0722 * linearize(b)
   where linearize(c) = c/12.92 if c<=0.04045 else ((c+0.055)/1.055)^2.4
4. If L > 0.179 → fg_color = "#000000" (dark text on light bg)
   If L ≤ 0.179 → fg_color = "#FFFFFF" (light text on dark bg)
5. Derive overlay_bg: bg_color with 85% opacity for readability
```

---

### 5.6 IPC Payload — Rust → Tauri Frontend

**Format:** JSON array emitted via Tauri `emit()` event on channel `"translation-update"`.

**Schema:**

```typescript
interface TranslationBox {
  id: string; // Unique ID (e.g., UUID or frame_id + index)
  translated: string; // English translation
  original: string; // Original Japanese text (for tooltip/debug)
  x: number; // Left edge in logical CSS pixels
  y: number; // Top edge in logical CSS pixels
  width: number; // Box width in logical CSS pixels
  height: number; // Box height in logical CSS pixels
  bg_color: string; // CSS rgba string, e.g. "rgba(30, 30, 30, 0.85)"
  fg_color: string; // "#000000" or "#FFFFFF"
  confidence: number; // OCR confidence 0.0 – 1.0
}

type TranslationPayload = {
  boxes: TranslationBox[];
  scale_factor: number; // Retina scale (1.0 or 2.0)
  frame_id: number; // Monotonically increasing frame counter
};
```

**Clear Event:** Emit `"translation-clear"` with no payload when SCROLLING state is entered.

---

### 5.7 Tauri Overlay Window

**Window Configuration (tauri.conf.json):**

```json
{
  "width": "<full_display_width>",
  "height": "<full_display_height>",
  "transparent": true,
  "decorations": false,
  "alwaysOnTop": true,
  "resizable": false,
  "skipTaskbar": true,
  "shadow": false
}
```

**macOS-Specific Rust Setup:**

```rust
window.set_ignore_cursor_events(true)?;  // Click-through
window.set_content_protection(false)?;   // Allow ScreenCaptureKit to see it (avoid capture loop)
```

**Capture Loop Prevention:**
The overlay window itself must be **excluded** from the ScreenCaptureKit capture stream using `SCStreamConfiguration.excludedWindows`, otherwise the overlay will be captured, OCR'd, and translated in an infinite loop.

**Frontend Rendering (JavaScript):**

- Listen for `translation-update` event
- Clear all existing `.translation-box` elements
- For each box in payload, create an absolutely-positioned `<div>`:
  ```html
  <div
    class="translation-box"
    style="
    position: absolute;
    left: {x}px; top: {y}px;
    width: {width}px; height: {height}px;
    background: {bg_color};
    color: {fg_color};
    font-size: clamp(10px, {height * 0.7}px, 24px);
    border-radius: 3px;
    padding: 2px 4px;
    pointer-events: none;
  "
  >
    {translated}
  </div>
  ```
- Listen for `translation-clear` event → remove all boxes immediately

---

## 6. Debug CLI Mode (`--debug-cli`)

**Purpose:** Allow the full Rust engine (capture → motion → OCR → translation → styling) to be tested entirely from the terminal, bypassing Tauri and the WebView completely. Essential for performance profiling and debugging without fighting a transparent overlay.

**Activation:** `cargo run -- --debug-cli`

**Behavior in this mode:**

- Tauri window is **not created**
- ScreenCaptureKit capture loop starts normally
- Motion detection and debounce run normally
- On trigger: OCR + translation + styling run normally
- Output is printed as pretty-printed JSON to `stdout`:

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
      "bg_color": "rgba(255,255,255,0.85)",
      "fg_color": "#000000",
      "confidence": 0.97
    }
  ]
}
```

**Additional CLI flags:**
| Flag | Description |
|---|---|
| `--debug-cli` | Headless mode, JSON to stdout |
| `--debug-cli --pretty` | Pretty-print JSON output |
| `--debug-cli --once` | Trigger exactly one OCR cycle then exit (useful for scripting) |
| `--list-models` | Print model manifest table and exit |
| `--prune-models` | Interactive CLI model cleanup wizard |

**Why this matters:** Debugging a transparent click-through window with browser DevTools requires the app to be in focus — which is awkward when the whole point is that it's click-through. The CLI mode lets you run the engine, pipe output to `jq`, and inspect results without any UI friction.

---

## 7. Hotkey & Controls

| Hotkey            | Action                                                   |
| ----------------- | -------------------------------------------------------- |
| `Cmd + Shift + T` | Toggle overlay visibility on/off                         |
| `Cmd + Shift + Q` | Quit application                                         |
| `Cmd + Shift + R` | Force re-trigger OCR on current screen (bypass debounce) |

Hotkeys must be registered as global shortcuts (work even when app is not focused) via Tauri's `globalShortcut` plugin.

---

## 7. Memory Budget

| Component                       | Default (NLLB) | Quality Mode (Gemma 4 E4B) |
| ------------------------------- | -------------- | -------------------------- |
| Translation model               | ~1.2 GB        | ~5.0 GB                    |
| Tauri WebView (frontend)        | ~80 MB         | ~80 MB                     |
| Rust backend (buffers, state)   | ~50 MB         | ~50 MB                     |
| Apple Vision (ANE, no RAM pool) | ~0 MB          | ~0 MB                      |
| Frame buffer (2× 4K BGRA)       | ~96 MB         | ~96 MB                     |
| **Total Estimated**             | **~1.5 GB**    | **~5.3 GB**                |
| **Hard Ceiling**                | **3.0 GB**     | **8.0 GB**                 |

> Quality Mode requires closing memory-heavy apps (browsers with many tabs, etc.) on a 16GB system to avoid swapping.

---

## 8. Security & Privacy

- All processing is **fully local** — no data leaves the device
- The app captures screen contents; this is disclosed in onboarding
- No telemetry, no analytics, no network requests after model download
- The model file should be stored in `~/Library/Application Support/jp-translate/models/`

---

## 9. Error Handling & Edge Cases

| Scenario                         | Behavior                                                                              |
| -------------------------------- | ------------------------------------------------------------------------------------- |
| No Japanese text on screen       | OCR returns empty array; overlay clears                                               |
| Screen permission denied         | App shows permission request UI and blocks startup                                    |
| Model file not found             | Startup fails with a clear error dialog and download link                             |
| Translation takes > 5s           | Show "Translating…" placeholder; timeout and show original Japanese                   |
| App is capturing itself          | Excluded window prevents feedback loop                                                |
| User switches display resolution | Re-query display scale_factor on `NSApplicationDidChangeScreenParametersNotification` |

---

## 10. Risk Register

| Risk                                             | Severity | Mitigation                                                                                                                      |
| ------------------------------------------------ | -------- | ------------------------------------------------------------------------------------------------------------------------------- |
| `objc2-vision` bindings are incomplete or broken | High     | Prepare fallback: call Vision via a Swift helper binary via `std::process::Command`, or use `tesseract-rs` as degraded fallback |
| NLLB via llama.cpp produces poor quality         | Medium   | Validate translation quality during Phase 3 before full integration; switch to Gemma 4 E4B Quality Mode for better results      |
| Gemma 4 E4B causes RAM pressure in Quality Mode  | Medium   | Warn user before switching; show current RAM usage in menu bar during Quality Mode                                              |
| Overlay capture loop                             | High     | Explicitly exclude overlay window from SCStream at initialization                                                               |
| Retina coordinate mismatch                       | Medium   | Implement `scale_factor` normalization from day one; test on Retina display in Phase 1                                          |
| macOS notarization requirements                  | Low      | Use Tauri's built-in signing/notarization toolchain; plan for Apple Developer account                                           |
