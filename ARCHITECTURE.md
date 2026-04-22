# Contextura — Architecture

Real-time Japanese→English screen translation overlay for macOS (Apple Silicon).

**Version:** 1.5.0  
**Stack:** Rust · Tauri v2 · Swift `vision-helper` · `llama-server` sidecar · Vanilla HTML/CSS/JS  
**Last Updated:** 2026-04-22

---

## System Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                     TAURI APPLICATION                        │
│                                                              │
│  ┌─────────────────────────┐   ┌────────────────────────┐  │
│  │    Rust Backend          │   │  WebView Overlay        │  │
│  │    (lib.rs orchestrates) │   │  (HTML/CSS/Vanilla JS)  │  │
│  │                          │   │                          │  │
│  │  capture.rs (SCKit)      │   │  overlay.js             │  │
│  │       │                  │   │  Listens for:            │  │
│  │  motion.rs               │   │  • translation-update   │  │
│  │  (160×90 diff + debounce)│   │  • translation-clear    │  │
│  │       │ Triggered        │   │  • translation-started  │  │
│  │  save_frame_as_png()     │   │  • translation-error    │  │
│  │       │                  │   │                          │  │
│  │  vision-helper (Swift)   │   │  Renders positioned     │  │
│  │  (Apple Vision OCR)      │   │  transparent divs over  │  │
│  │       │                  │   │  original Japanese text  │  │
│  │  styling.rs              │   └────────────────────────┘  │
│  │  (WCAG 2.1 Rayon)        │              ↑                 │
│  │       │                  │    app_handle.emit()           │
│  │  translation.rs          │    "translation-update"        │
│  │  (llama-server HTTP)     │                                │
│  │       │                  │                                │
│  │  ipc.rs (TranslationPayload) ──────────────────────────► │
│  └─────────────────────────┘                                │
│                                                              │
│  Sidecars:                                                   │
│  • llama-server-aarch64-apple-darwin (Qwen3-0.6B, Metal)   │
│  • vision-helper-aarch64-apple-darwin (Apple Vision API)    │
└─────────────────────────────────────────────────────────────┘
```

---

## Data Flow (End-to-End)

```
1.  SCStream → OutputHandler::did_output_sample_buffer()
    → CaptureFrame { BGRA pixels, width, height, scale_factor }
    → crossbeam::bounded(2) channel

2.  MotionDetector::downsample()
    → 160×90 grayscale thumbnail (nearest-neighbour)

3.  MotionDetector::process_thumbnail()
    → compute_diff_mask() → largest_contiguous_region() → motion_ratio: f32

4.  DebounceStateMachine::update(motion_ratio)
    → MotionDetected  → emit("translation-clear")
    → Triggered       → proceed to step 5
    → None            → continue (discard frame)

5.  save_frame_as_png(frame, frame_id)
    → BGRA → RGBA channel swap (swap index 0 ↔ 2)
    → image::ImageBuffer::from_raw() → save to /tmp/contextura-frame-{id}.png

6.  emit("translation-started", { display_id: 0 })

7.  OcrEngine::recognize(&png_path, width, height, scale_factor)
    → Command::new(vision-helper).arg(png_path).output()
    → Parse JSON array from stdout
    → Coordinate conversion: bottom-left → top-left logical CSS points
    → Furigana suppression (height < 40% of overlapping parent)
    → Confidence filter (< 0.4 dropped), CJK filter, IoU merge (> 0.3)

8.  std::fs::remove_file(&png_path)  ← always, even on error

9.  Drain invalidation_rx:
    → AppSwitch  → memory.clear() + emit("translation-clear")
    → ManualReset → memory.clear() (overlay stays visible)
    → ModelSwitch → memory.clear() + emit("translation-clear")

10. TranslationClient::translate_batch(&texts)
    → POST http://127.0.0.1:8765/v1/chat/completions
    → Batched numbered prompt + rolling context header (6 entries max)
    → Sub-batch at 15 strings
    → Parse ^(\d+): (.+)$ per line

11. StylingEngine::sample_rect_ring() per box (Rayon par_iter)
    → Sample 2px outer ring of bounding box → average RGBA
    → WCAG 2.1 relative luminance → #000000 or #FFFFFF foreground
    → rgba(r, g, b, 0.85) background

12. Build TranslationPayload { boxes: Vec<TranslationBox>, ... }
    → app_handle.emit("translation-update", &payload)

13. overlay.js renders absolutely-positioned divs over original text
```

---

## Module Reference

| File             | Responsibility                                        | Status                                  |
| ---------------- | ----------------------------------------------------- | --------------------------------------- |
| `lib.rs`         | App entry, Tauri setup, pipeline orchestration        | ✅ Wired (Phase P.Complete)             |
| `main.rs`        | Thin passthrough to `app_lib::run()`                  | ✅                                      |
| `capture.rs`     | ScreenCaptureKit frame capture, display 0             | ✅                                      |
| `motion.rs`      | 160×90 motion detection + debounce state machine      | ✅ Wired                                |
| `ocr.rs`         | vision-helper subprocess wrapper + post-processing    | ✅ Wired                                |
| `translation.rs` | llama-server HTTP client + rolling context memory     | ✅ Wired                                |
| `styling.rs`     | WCAG 2.1 luminance + RGBA sampling                    | ✅ Wired                                |
| `ipc.rs`         | `TranslationBox` / `TranslationPayload` structs       | ✅ Emitted                              |
| `context.rs`     | NSWorkspace app-switch tracker + invalidation channel | ✅ Wired                                |
| `thermal.rs`     | IOKit thermal state monitor                           | ✅ (battery check hardcoded `false`)    |
| `hotkeys.rs`     | Global keyboard shortcuts                             | ⚠️ T, R stubs; Q, M working             |
| `tray.rs`        | System tray menu                                      | ⚠️ Structure OK; most handlers log-only |
| `settings.rs`    | settings.json read/write + defaults                   | ✅                                      |
| `cli.rs`         | CLI arg parsing (`--debug-cli`, `--list-models`)      | ✅ (outputs are stubs)                  |
| `downloader.rs`  | Model downloader                                      | ❌ Never called                         |

---

## Sidecar Architecture

### llama-server (`binaries/llama-server-aarch64-apple-darwin`)

Pre-compiled from `llama.cpp` for Apple Silicon. Launched via `tauri-plugin-shell`.

**Launch args:**

```
--model <path_to_qwen3.gguf>
--port 8765
--n-gpu-layers 99        # Full Metal GPU offload
--ctx-size 1024
--host 127.0.0.1
--log-disable
--jinja                  # Required for Qwen3 chat template
```

**Health check:** `GET http://127.0.0.1:8765/health` → `{"status":"ok"}`

**Model:** Qwen3-0.6B Q4_K_M (~350MB). Must be a **decoder-only** model — llama-server does not support encoder-decoder architectures (NLLB, T5, BART).

**Translation request format:**

```json
{
  "model": "local",
  "messages": [
    {
      "role": "system",
      "content": "You are a Japanese-to-English translator. /no_think"
    },
    { "role": "user", "content": "1: こんにちは\n2: ありがとう" }
  ],
  "temperature": 0.1,
  "max_tokens": 512
}
```

The `/no_think` token disables Qwen3's thinking mode — without it, the model outputs `<think>...</think>` tokens that break the numbered-line response parser.

### vision-helper (`binaries/vision-helper-aarch64-apple-darwin`)

Lightweight Swift CLI wrapping `Vision.VNRecognizeTextRequest`. Accepts a PNG path argument, writes a JSON array to stdout:

```json
[
  {
    "text": "日本語",
    "confidence": 0.97,
    "x": 0.12,
    "y": 0.45,
    "width": 0.3,
    "height": 0.04,
    "text_angle": 0.0
  }
]
```

Coordinates are **bottom-left origin, normalized** (Vision framework default). `ocr.rs` converts to top-left CSS points.

---

## Frontend

Three files, zero build steps, zero frameworks:

| File              | Responsibility                                        |
| ----------------- | ----------------------------------------------------- |
| `src/index.html`  | Overlay page (transparent, borderless, click-through) |
| `src/overlay.js`  | IPC listeners + DOM rendering of translation boxes    |
| `src/overlay.css` | Overlay styles (absolute positioning, typography)     |
| `src/wizard.html` | First-run setup (Screen 1: permission request only)   |
| `src/help.html`   | Help page                                             |

**IPC events (all wired as of Phase P.Complete):**

| Event                 | Payload              | Handler                 |
| --------------------- | -------------------- | ----------------------- |
| `translation-update`  | `TranslationPayload` | Renders boxes into DOM  |
| `translation-clear`   | —                    | Clears all overlay divs |
| `translation-started` | `{ display_id }`     | Shows loading indicator |
| `translation-error`   | `{ message }`        | Shows error state       |

---

## Capability Requirements

`src-tauri/capabilities/default.json` must include:

```json
"permissions": [
  "core:default",
  "core:window:allow-close",
  "core:webview:allow-webview-close",
  "shell:allow-execute",
  "shell:allow-spawn"
]
```

`shell:allow-execute` and `shell:allow-spawn` are required for `app.shell().sidecar()` to succeed. Without them, Tauri v2 silently denies the call at runtime.

---

## Coordinate System

| Stage                     | Origin      | Unit                                 |
| ------------------------- | ----------- | ------------------------------------ |
| Vision framework output   | Bottom-left | Normalized (0.0–1.0)                 |
| After `ocr.rs` conversion | Top-left    | Logical CSS points                   |
| CSS overlay positioning   | Top-left    | Points (matching window coordinates) |

**Y-axis flip:** `css_y = (1.0 - vision_y - vision_height) * screen_height / scale_factor`

---

## Known Remaining Gaps

| Item                                  | Priority                                         |
| ------------------------------------- | ------------------------------------------------ |
| `Cmd+Shift+T` toggle (log stub)       | High — Phase P.Complete Step 6                   |
| `Cmd+Shift+R` force OCR (log stub)    | High — Phase P.Complete Step 6                   |
| Real `scale_factor` from display info | Medium — currently hardcoded 2.0 in `capture.rs` |
| `excludedWindows` in SCKit config     | Medium — prevent overlay capture loop            |
| Watchdog: poll `/health` every 5s     | Medium — Phase P.Complete Step 7                 |
| Battery check in `thermal.rs`         | Low — `IOPSCopyPowerSourcesInfo`                 |
| Wizard screens 2–4                    | Phase 8                                          |
