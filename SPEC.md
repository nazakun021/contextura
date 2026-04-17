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
4. Compute sum of absolute pixel differences vs. previous thumbnail
5. Normalize by pixel count → motion_ratio (0.0 – 1.0)
6. Apply threshold: motion_ratio > 0.05 → "motion detected"
```

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
| `MOTION_THRESHOLD` | 0.05 | Fraction of pixels changed to count as motion |
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

### 5.4 Translation Engine — llama.cpp + NLLB

**Purpose:** Translate extracted Japanese strings to English.

**Model:**

- **Primary:** `NLLB-200-distilled-600M` quantized to Q4_K_M format
  - Estimated VRAM/RAM: ~1.2GB
  - Language token for Japanese source: `__jpn_Jpan__`
  - Language token for English target: `__eng_Latn__`
- **Optional Quality Mode:** ALMA-7B-R at Q4_K_M (~3.8GB) — user-selectable

**Integration:**

- Use `llama_cpp_rs` crate (or direct `llama.cpp` C bindings via `bindgen`)
- Load model once at app startup; keep resident in memory for the entire session
- Use Metal backend (`n_gpu_layers = 99` to offload all layers to GPU)

**Translation Request Format:**

```
Translate the following Japanese text to English. Output only the translation, no explanations.
Japanese: {input_text}
English:
```

**Pipeline Parallelism:**

- After OCR produces N bounding boxes, group them into batches
- Use Rayon `par_iter()` to submit translation requests concurrently
- Cap concurrent inference threads to avoid memory contention: `max_parallel = 2`

**Performance Targets:**
| Metric | Target |
|---|---|
| Translation latency (single string, <50 chars) | < 800ms |
| Translation latency (full screen, ~10 strings) | < 3s |
| Model load time at startup | < 5s |
| Peak RAM usage (model + app) | < 2GB |

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

## 6. Hotkey & Controls

| Hotkey            | Action                                                   |
| ----------------- | -------------------------------------------------------- |
| `Cmd + Shift + T` | Toggle overlay visibility on/off                         |
| `Cmd + Shift + Q` | Quit application                                         |
| `Cmd + Shift + R` | Force re-trigger OCR on current screen (bypass debounce) |

Hotkeys must be registered as global shortcuts (work even when app is not focused) via Tauri's `globalShortcut` plugin.

---

## 7. Memory Budget

| Component                        | Estimated RAM |
| -------------------------------- | ------------- |
| NLLB-200-distilled-600M (Q4_K_M) | ~1.2 GB       |
| Tauri WebView (frontend)         | ~80 MB        |
| Rust backend (buffers, state)    | ~50 MB        |
| Apple Vision (ANE, no RAM pool)  | ~0 MB         |
| Frame buffer (2× 4K BGRA)        | ~96 MB        |
| **Total Estimated**              | **~1.5 GB**   |
| **Hard Ceiling**                 | **3.0 GB**    |

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
| NLLB via llama.cpp produces poor quality         | Medium   | Validate translation quality during Phase 2 before full integration; keep ALMA as quality-mode option                           |
| Overlay capture loop                             | High     | Explicitly exclude overlay window from SCStream at initialization                                                               |
| Retina coordinate mismatch                       | Medium   | Implement `scale_factor` normalization from day one; test on Retina display in Phase 1                                          |
| macOS notarization requirements                  | Low      | Use Tauri's built-in signing/notarization toolchain; plan for Apple Developer account                                           |
