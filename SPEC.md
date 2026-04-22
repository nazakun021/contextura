# SPEC.md — Contextura: Real-Time Screen Translation Overlay

**Version:** 1.5.0
**Target Platform:** macOS 13+ (Apple Silicon, M-series)
**Last Updated:** 2026-04-22

---

## Changelog

| Version | Changes                                                                                                                                                                                                                                                                                                                       |
| ------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1.0.0   | Initial specification                                                                                                                                                                                                                                                                                                         |
| 1.1.0   | Vertical text, furigana suppression, multi-monitor, crash recovery, thermal awareness, onboarding wizard, Sentry                                                                                                                                                                                                              |
| 1.2.0   | Consolidated structure; settings.json, RAM guard, privacy screen, in-app help, `--test-suite`                                                                                                                                                                                                                                 |
| 1.3.0   | Resolved architecture: Swift subprocess OCR, llama-server sidecar translation; Foundation Models v1.1 tier                                                                                                                                                                                                                    |
| 1.4.0   | Audit-accurate status; single-screen focus; documented pipeline gap in lib.rs                                                                                                                                                                                                                                                 |
| 1.5.0   | **NLLB removed.** llama.cpp does not support encoder-decoder (BART/seq2seq) architectures. Standard tier replaced with Qwen3-0.6B Q4_K_M (~350MB, decoder-only, Japanese-capable). Qwen3 thinking mode disable documented. llama-server args updated with `--jinja` flag. Translation system prompt updated with `/no_think`. |

---

## Critical Architecture Note — Why NLLB Was Replaced

NLLB-200 is an encoder-decoder (BART-based seq2seq) model. llama.cpp and its `llama-server` binary support **only decoder-only transformer architectures** (LLaMA, Mistral, Gemma, Qwen, etc.). This is a fundamental architectural incompatibility — not a configuration issue. When loaded, llama-server immediately exits with:

```
llama_model_load: error loading model: unknown model architecture: 'nllb'
```

**Resolution:** Replace NLLB with Qwen3-0.6B as the Standard tier. It is decoder-only, fully supported by llama-server, has native Japanese language capability, and its Q4_K_M quantization is approximately 350MB — smaller than NLLB was. The entire llama-server sidecar infrastructure, HTTP API calls, and parsing logic in `translation.rs` require no changes. Only the model file and two sidecar launch arguments change.

---

## Accurate Implementation Status (Audit-Verified 2026-04-22)

| Module / Feature                    | File                        | Status                 | Notes                                      |
| ----------------------------------- | --------------------------- | ---------------------- | ------------------------------------------ |
| Tauri scaffold + transparent window | `lib.rs`, `tauri.conf.json` | ✅ Working             |                                            |
| Settings JSON                       | `settings.rs`               | ✅ Working             |                                            |
| CLI flag parsing                    | `cli.rs`                    | ✅ Working             |                                            |
| ScreenCaptureKit frame capture      | `capture.rs`                | ✅ Working             | Display 0 only                             |
| Vision-helper Swift binary          | `binaries/vision-helper-*`  | ✅ Working             |                                            |
| OCR subprocess wrapper              | `ocr.rs`                    | ✅ Working             | Coordinate conversion, furigana, IoU merge |
| llama-server sidecar                | `binaries/llama-server-*`   | ✅ Working             | Needs decoder-only model file              |
| Translation HTTP client             | `translation.rs`            | ✅ Working             | Real HTTP, batching, context memory        |
| WCAG styling math                   | `styling.rs`                | ✅ Working             | Unit tests pass                            |
| IPC payload structs                 | `ipc.rs`                    | ✅ Working             |                                            |
| Frontend event listeners            | `overlay.js`                | ✅ Working             | All 4 events handled                       |
| System tray (menu structure)        | `tray.rs`                   | ✅ Working             | Handlers mostly log stubs                  |
| Motion detection logic              | `motion.rs`                 | ✅ Code correct        | **Never instantiated in lib.rs**           |
| Debounce state machine              | `motion.rs`                 | ✅ Code correct        | **Never instantiated in lib.rs**           |
| Context invalidation tracker        | `context.rs`                | ✅ NSWorkspace polling | `memory.clear()` not wired                 |
| Thermal monitor                     | `thermal.rs`                | ⚠️ Partial             | Battery hardcoded `false`                  |
| **Pipeline orchestration**          | `lib.rs`                    | ❌ Broken              | Subsystems exist; none connected           |
| PNG snapshot on trigger             | `lib.rs`                    | ❌ Missing             | Frame never written to disk                |
| Motion detection in frame loop      | `lib.rs`                    | ❌ Missing             |                                            |
| Styling in pipeline                 | `lib.rs`                    | ❌ Missing             |                                            |
| IPC event emission                  | `lib.rs`                    | ❌ Missing             | `app_handle.emit()` never called           |
| Shell capability for sidecar        | `capabilities/default.json` | ❌ Missing             | `shell:allow-execute` absent               |
| Correct model file                  | local filesystem            | ❌ Wrong arch          | NLLB replaced by Qwen3-0.6B                |
| Hotkey: toggle overlay              | `hotkeys.rs`                | ❌ Log stub            |                                            |
| Hotkey: force OCR                   | `hotkeys.rs`                | ❌ Log stub            |                                            |
| Watchdog (health poll)              | —                           | ❌ Not implemented     |                                            |

---

## 1. Project Overview

A high-performance desktop overlay application that detects Japanese text on-screen, translates it to English in real-time using local AI models, and renders translated text as a transparent, click-through overlay precisely positioned over the original text. Runs entirely offline after model download.

**Current development focus:** Wire the pipeline in `lib.rs` on a single display. All individual subsystems work correctly in isolation.

---

## 2. Technology Stack

| Layer                        | Technology                          | Status                   |
| ---------------------------- | ----------------------------------- | ------------------------ |
| App Framework                | Tauri v2.x                          | ✅                       |
| Backend Language             | Rust 1.78+                          | ✅                       |
| Screen Capture               | `screencapturekit` Rust crate       | ✅ (display 0)           |
| OCR                          | Swift `vision-helper` subprocess    | ✅                       |
| Translation Runtime          | `llama-server` sidecar + HTTP       | ✅ (needs correct model) |
| Translation Model (Standard) | **Qwen3-0.6B Q4_K_M** ~350MB        | ⚠️ Needs download        |
| Translation Model (Quality)  | Gemma 4 E4B IT Q4_K_M ~5GB          | ❌ Deferred              |
| Translation (Native, v1.1)   | Apple Foundation Models (macOS 26+) | ❌ v1.1                  |
| Parallelism                  | Rayon                               | ❌ Not yet called        |
| Frontend                     | HTML5 / CSS3 / Vanilla JS           | ✅                       |
| Crash Reporting              | sentry-rust (opt-in)                | ❌ Not wired             |
| HTTP Client                  | reqwest                             | ✅                       |

---

## 3. Subsystem Specifications

### 3.1 Screen Capture — ScreenCaptureKit

**Status:** ✅ Working for display 0.

Real `SCStream` with `OutputHandler` delivering `CaptureFrame` to `crossbeam::bounded(2)`.
`excludedWindows` not yet set (low risk for development; required before Phase 8).

---

### 3.2 Motion Detection

**Status:** ✅ Logic in `motion.rs`. ❌ Never instantiated in `lib.rs`.

**Algorithm:**

```
1. Receive CaptureFrame
2. Downscale to 160x90 grayscale
3. Exclude 5% edge inset
4. Per-pixel diff mask (threshold: pixel_diff_threshold = 15)
5. 4-connected union-find connected-components
6. motion_ratio = largest_contiguous_region / total_comparison_area
7. motion_ratio > motion_threshold (0.05) -> "motion detected"
```

**Debounce:** `SCROLLING | SETTLING(Instant) | IDLE`

**Required wiring in `lib.rs`:** Instantiate `MotionDetector` + `DebounceStateMachine` before frame loop; feed every frame through them; branch on `DebounceEvent`.

---

### 3.3 OCR — vision-helper Subprocess

**Status:** ✅ Working when called with a valid PNG path. ❌ Currently called with non-existent path because frame is never saved.

**Subprocess call (implemented in `ocr.rs`):**

```rust
Command::new(vision_helper_path).arg(png_path).output()
```

**Output JSON from Swift binary:**

```json
[
  {
    "text": "...",
    "confidence": 0.97,
    "x": 0.12,
    "y": 0.45,
    "width": 0.3,
    "height": 0.04,
    "text_angle": 0.0
  }
]
```

**Post-processing (all working in `ocr.rs`):** Y-axis flip, `scale_factor` division, `is_vertical` from `text_angle`, furigana suppression, confidence/CJK/IoU filters.

**Required fix in `lib.rs`:** Save frame buffer as PNG before calling `ocr_engine.recognize()`. Delete temp file after.

---

### 3.4 Translation Engine — llama-server + Qwen3-0.6B

**Status:** ✅ HTTP client works. ✅ Sidecar launches correctly. ⚠️ Requires Qwen3-0.6B model file (not NLLB).

#### Model Tiers

| Tier              | Model                   | Size (Q4_K_M) | RAM    | Minimum OS |
| ----------------- | ----------------------- | ------------- | ------ | ---------- |
| **Standard**      | **Qwen3-0.6B**          | **~350 MB**   | Low    | macOS 13+  |
| **Quality Mode**  | Gemma 4 E4B IT          | ~5 GB         | Higher | macOS 13+  |
| **Native (v1.1)** | Apple Foundation Models | ~0 GB         | None   | macOS 26+  |

#### Why Qwen3-0.6B

- Decoder-only transformer — fully supported by llama-server
- Native Japanese language support
- ~350MB Q4_K_M — smaller than NLLB
- Fast on Apple Silicon Metal via `-ngl 99`
- OpenAI-compatible API — no changes to `translation.rs`
- Available on Hugging Face: `Qwen/Qwen3-0.6B-GGUF`

#### Sidecar Launch Args — Updated for Qwen3

```
llama-server
  --model <path_to_qwen3-0.6B.Q4_K_M.gguf>
  --port 8765
  --n-gpu-layers 99
  --ctx-size 1024
  --host 127.0.0.1
  --log-disable
  --jinja                  ← REQUIRED for Qwen3 chat template
```

The `--jinja` flag is **required** for Qwen3 models. It tells llama-server to use the model's embedded chat template (Jinja2 format) rather than a generic prompt format. Without it, Qwen3 may not follow the system prompt correctly.

#### Qwen3 Thinking Mode — Must Be Disabled

Qwen3 has a built-in "thinking mode" that outputs `<think>...</think>` tokens before answering. For translation this must be explicitly disabled, otherwise it breaks the `^(\d+): (.+)$` numbered-line parsing in `translation.rs`.

**Disable by adding `/no_think` to the system prompt:**

```
You are a Japanese-to-English translator. /no_think
```

This instructs the model to skip the thinking step and respond directly with translations.

#### Translation Request Format — Batched Single-Pass with Rolling Context

```
System: You are a Japanese-to-English translator. /no_think

User:
[If context memory non-empty:]
Previous context (do not retranslate, for reference only):
- {memory[0].ja} -> "{memory[0].en}"
...up to 6 entries

Translate each numbered Japanese string to English.
Output only the translations, one per line, in the same numbered format.

1: {ocr_result[0].text}
2: {ocr_result[1].text}
...
```

**Output parsing (unchanged in `translation.rs`):**

- Match `^(\d+): (.+)$` per response line
- Map back to `OcrResult` by index
- `""` for missing/malformed lines
- Sub-batch at 15 strings

#### Performance Targets

| Metric                            | Qwen3-0.6B (Standard) | Gemma 4 E4B (Quality) |
| --------------------------------- | --------------------- | --------------------- |
| Translation latency (~10 strings) | < 2s                  | < 5s                  |
| Sidecar startup time              | < 3s                  | < 8s                  |
| Peak RAM (model + app)            | < 1GB                 | < 6.5GB               |

---

### 3.5 Dynamic Styling

**Status:** ✅ Logic correct. ❌ Never called from `lib.rs`.

WCAG 2.1 luminance formula. `L > 0.179` → black text; `L ≤ 0.179` → white text. 85% opacity background.

---

### 3.6 IPC Payload

**Status:** ✅ Structs defined. ❌ `app_handle.emit()` never called.

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

| Event                   | When                       | Status           |
| ----------------------- | -------------------------- | ---------------- |
| `"translation-started"` | Before `translate_batch()` | ❌ Never emitted |
| `"translation-update"`  | After batch + styling      | ❌ Never emitted |
| `"translation-clear"`   | On motion / app switch     | ❌ Never emitted |
| `"translation-error"`   | On watchdog restart        | ❌ Never emitted |

---

### 3.7 Rolling Translation Memory

**Status:** ✅ Struct + methods implemented. ❌ `clear()` not wired to invalidation events.

```rust
struct TranslationMemory {
    entries: VecDeque<(String, String)>,
    max_size: usize,  // default 6
}
```

---

### 3.8 Context Invalidation

**Status:** ✅ `AppWindowTracker` polls NSWorkspace. ❌ `memory.clear()` not called on switch.

| Trigger       | Clears Memory | Clears Overlay |
| ------------- | ------------- | -------------- |
| App switch    | Yes           | Yes            |
| `Cmd+Shift+M` | Yes           | No             |
| Model switch  | Yes           | Yes            |
| Scroll/motion | No            | Yes            |

---

### 3.9 Pipeline Orchestration — lib.rs (The Gap)

**Current broken flow:**

```
1. ✅ Start sidecar → wait for /health
2. ✅ Start capture display 0
3. ✅ Receive frame
4. ❌ No motion detection
5. ❌ OCR called with non-existent PNG
6. ⚠️ Translations logged but not used
7. ❌ No styling
8. ❌ No emit()
```

**Target working flow:**

```
Before loop:
  - Instantiate MotionDetector(settings)
  - Instantiate DebounceStateMachine(debounce_ms)
  - Instantiate StylingEngine

Loop:
  frame = frame_rx.recv()
  motion_ratio = motion_detector.process(&frame)

  match debounce.update(motion_ratio):
    MotionDetected ->
      emit("translation-clear")

    Triggered ->
      frame_id += 1
      png_path = save_frame_as_png(&frame, frame_id)  // image crate
      emit("translation-started")
      ocr_results = ocr_engine.recognize(&png_path)
      delete_temp_png(&png_path)
      if ocr_results.is_empty() -> continue

      drain invalidation_rx:
        AppSwitch -> memory.clear() + emit("translation-clear")
        ManualReset -> memory.clear()
        ModelSwitch -> memory.clear() + emit("translation-clear")

      translations = translation_client.translate_batch(
          &texts, memory.as_context_slice()
      )
      styled_boxes = rayon::par_iter over (ocr, translation) -> TranslationBox
      payload = TranslationPayload { boxes, scale_factor, display_id: 0, frame_id }
      emit("translation-update", &payload)
      memory.push_all(ocr_results, translations)

    NoChange -> continue
```

---

## 4. Model Download & Manifest

### Model Storage Path

`~/Library/Application Support/contextura/models/`

### Updated Manifest (Qwen3-0.6B replaces NLLB)

```json
{
  "models": [
    {
      "id": "qwen3-0.6b-q4",
      "filename": "qwen3-0.6b-q4_k_m.gguf",
      "size_bytes": 370000000,
      "sha256": "<verify from HuggingFace>",
      "downloaded_at": "2026-04-22T00:00:00Z",
      "last_used_at": "2026-04-22T00:00:00Z",
      "active": true
    },
    {
      "id": "gemma4-e4b-q4",
      "filename": "gemma-4-e4b-it.Q4_K_M.gguf",
      "size_bytes": 5368709120,
      "sha256": "<verify from HuggingFace>",
      "downloaded_at": "2026-04-22T00:00:00Z",
      "last_used_at": "2026-04-22T00:00:00Z",
      "active": false
    }
  ]
}
```

**Download command:**

```bash
huggingface-cli download Qwen/Qwen3-0.6B-GGUF \
  qwen3-0.6b-q4_k_m.gguf \
  --local-dir ~/Library/Application\ Support/contextura/models/
```

---

## 5. Settings File

**Path:** `~/Library/Application Support/contextura/settings.json`

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
  "active_model": "qwen3-0.6b-q4"
}
```

---

## 6. Hotkey & Controls

| Hotkey            | Status      | Notes                          |
| ----------------- | ----------- | ------------------------------ |
| `Cmd + Shift + T` | ❌ Log stub | Needs `window.show()`/`hide()` |
| `Cmd + Shift + Q` | ✅ Working  |                                |
| `Cmd + Shift + R` | ❌ Log stub | Needs force-trigger channel    |
| `Cmd + Shift + M` | ✅ Working  | Calls `trigger_manual_reset()` |
| `Cmd + Shift + G` | ❌ Log stub | No `switch_model()` yet        |

---

## 7. Known Issues — Priority Ordered

| Priority | Issue                              | Fix                                                               |
| -------- | ---------------------------------- | ----------------------------------------------------------------- |
| 1        | Shell capability missing           | Add `shell:allow-execute` to `capabilities/default.json`          |
| 2        | Wrong model architecture (NLLB)    | Download Qwen3-0.6B Q4_K_M; update manifest + settings            |
| 3        | PNG never written                  | Use `image` crate in `lib.rs` to encode frame before OCR          |
| 4        | Motion detection not instantiated  | Instantiate `MotionDetector` + `DebounceStateMachine` in `lib.rs` |
| 5        | No IPC events emitted              | Call `app_handle.emit()` for all 4 event types                    |
| 6        | Styling never called               | Call `StylingEngine` before building payload                      |
| 7        | Context `memory.clear()` not wired | Drain `invalidation_rx` in loop                                   |
| 8        | Temp PNG not deleted               | `std::fs::remove_file()` after OCR                                |
| 9        | `Cmd+Shift+T` non-functional       | `window.show()`/`hide()`                                          |
| 10       | Battery check hardcoded false      | Real `IOPSCopyPowerSourcesInfo`                                   |

---

## 8. Memory Budget

| Component              | Standard (Qwen3-0.6B) | Quality (Gemma 4) |
| ---------------------- | --------------------- | ----------------- |
| Translation model      | ~0.35 GB              | ~5.0 GB           |
| llama-server process   | ~50 MB                | ~50 MB            |
| Tauri WebView          | ~80 MB                | ~80 MB            |
| Rust backend + buffers | ~150 MB               | ~150 MB           |
| **Total Estimated**    | **~0.65 GB**          | **~5.3 GB**       |
| **Hard Ceiling**       | **2.0 GB**            | **8.0 GB**        |

The Standard tier is now considerably lighter than the original NLLB spec. A 16GB M2 system should have no memory pressure in Standard mode even with a full browser session open.

---

## 9. Risk Register (Current Phase)

| Risk                                       | Severity | Mitigation                                                      |
| ------------------------------------------ | -------- | --------------------------------------------------------------- |
| Shell capability blocks sidecar spawn      | High     | Fix `capabilities/default.json` first                           |
| Qwen3 thinking mode not disabled           | High     | Add `/no_think` to system prompt; add `--jinja` to sidecar args |
| `image` crate BGRA→PNG wrong channel order | Medium   | Swap B↔R channels before encoding (BGRA → RGBA)                 |
| Temp PNG path collision                    | Medium   | Use frame_id in filename                                        |
| Overlay captured by SCKit                  | Low      | Fix `excludedWindows` before Phase 8                            |
| Context memory not wired                   | Low      | Easy fix; test with app switch post-pipeline                    |
