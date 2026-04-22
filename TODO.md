# TODO.md — Contextura: Real-Time Screen Translation Overlay

**Stack:** Rust · Tauri v2 · Swift vision-helper · llama-server sidecar · Qwen3-0.6B
**Platform:** macOS 13+ (Apple Silicon)
**Last Updated:** 2026-04-22

---

## Status Legend

- `[x]` — Verified working
- `[-]` — Code exists but stub/disconnected
- `[ ]` — Not started

---

## ⚠️ Model Change Notice

**NLLB has been removed from this project.** NLLB is an encoder-decoder (BART/seq2seq) architecture. llama.cpp supports **only decoder-only models**. This is not fixable via configuration.

**New Standard tier:** Qwen3-0.6B Q4_K_M (~350MB, decoder-only, Japanese-capable, fully llama.cpp compatible)

**No other code changes are required** — the HTTP API, payload format, and all `translation.rs` parsing logic remain identical. Only the model file and two sidecar launch args change.

---

## Pre-Flight Checks (Updated — Do Before Writing Any Code)

- [x] **Remove or rename the NLLB model file**
  - `rm "~/Library/Application Support/contextura/models/nllb-200-distilled-600M.Q4_K_M.gguf"`
  - It will never load with this sidecar — keeping it wastes 1.2GB

- [x] **Download Qwen3-0.6B Q4_K_M**

  ```bash
  huggingface-cli download Qwen/Qwen3-0.6B-GGUF \
    qwen3-0.6b-q4_k_m.gguf \
    --local-dir ~/Library/Application\ Support/contextura/models/
  ```

  Verify: file size should be ~350MB

- [x] **Update `manifest.json`**
  - Change `active_model` entry to:
    ```json
    {
      "id": "qwen3-0.6b-q4",
      "filename": "qwen3-0.6b-q4_k_m.gguf",
      "active": true
    }
    ```

- [x] **Update `settings.json`**
  - Change `active_model` to `"qwen3-0.6b-q4"`

- [x] **Test llama-server manually with Qwen3**

  ```bash
  ./src-tauri/binaries/llama-server-aarch64-apple-darwin \
    --model ~/Library/Application\ Support/contextura/models/qwen3-0.6b-q4_k_m.gguf \
    --port 8765 \
    --n-gpu-layers 99 \
    --ctx-size 1024 \
    --host 127.0.0.1 \
    --jinja
  ```

  In another terminal:

  ```bash
  curl http://127.0.0.1:8765/health
  ```

  Expected: `{"status":"ok"}`

- [x] **Test a translation manually**

  ```bash
  curl -X POST http://127.0.0.1:8765/v1/chat/completions \
    -H "Content-Type: application/json" \
    -d '{
      "model": "local",
      "messages": [
        {"role": "system", "content": "You are a Japanese-to-English translator. /no_think"},
        {"role": "user", "content": "Translate each numbered Japanese string to English.\nOutput only translations, one per line, same numbered format.\n\n1: こんにちは\n2: ありがとうございます"}
      ],
      "temperature": 0.1,
      "max_tokens": 256
    }'
  ```

  Expected: response containing `1: Hello` and `2: Thank you`

- [x] **Fix `capabilities/default.json`**
  - Path: `src-tauri/capabilities/default.json`
  - Add:
    ```json
    {
      "permissions": [
        "core:default",
        "shell:allow-execute",
        "shell:allow-spawn"
      ]
    }
    ```

- [x] **Test vision-helper**
  - Save a real Japanese screenshot as `/tmp/test-jp.png`
  - Run: `./src-tauri/binaries/vision-helper-aarch64-apple-darwin /tmp/test-jp.png`
  - Expected: JSON array with text/confidence/coordinates

**✅ Pre-flight complete when:** llama-server returns `{"status":"ok"}`, manual translation curl returns real English strings, vision-helper returns real JSON.

---

## Required Code Changes Before Pipeline Wiring

These are small but must be done before `lib.rs` wiring works.

### C.1 — Update Sidecar Args in `lib.rs` or `translation.rs`

Add `--jinja` to the llama-server launch args:

- [ ] Find where `llama-server` is spawned (likely `translation.rs` or `lib.rs`)
- [ ] Add `"--jinja"` to the args array
- [ ] Verify `--jinja` is present when the sidecar spawns (check logs)

### C.2 — Update System Prompt in `translation.rs`

Add `/no_think` to the system prompt to disable Qwen3 thinking mode:

- [ ] Find the system prompt string in `translation.rs`
- [ ] Change it to: `"You are a Japanese-to-English translator. /no_think"`
- [ ] Without this, Qwen3 will output `<think>...</think>` tokens that break parsing

### C.3 — Verify `active_model` Path Resolution

- [ ] Trace how `settings.active_model` maps to the actual `.gguf` file path
- [ ] Confirm the resolved path is passed correctly as `--model` to the sidecar
- [ ] Test: log the model path on startup before the sidecar launches

---

## Phase P.Complete — Wire the Pipeline in `lib.rs`

**The individual subsystems all work.** This connects them.
**Complete steps in order — verify each before proceeding.**

---

### Step 1 — Write PNG Snapshot on Debounce Trigger

**Problem:** `lib.rs` passes `/tmp/contextura-frame-latest.png` to OCR but never writes this file. The `image` crate is in `Cargo.toml` but never used.

**Fix:**

- [x] Add to `lib.rs` imports:
  ```rust
  use image::{ImageBuffer, RgbaImage};
  use std::path::PathBuf;
  ```
- [x] Add helper function:
  ```rust
  fn save_frame_as_png(frame: &CaptureFrame, frame_id: u64) -> anyhow::Result<PathBuf> {
      let path = PathBuf::from(format!("/tmp/contextura-frame-{}.png", frame_id));
      // CaptureFrame pixel buffer is BGRA — swap B and R to get RGBA
      let mut rgba_data = frame.data.clone();
      for pixel in rgba_data.chunks_exact_mut(4) {
          pixel.swap(0, 2);  // swap index 0 (B) with index 2 (R)
      }
      let img: RgbaImage = ImageBuffer::from_raw(frame.width, frame.height, rgba_data)
          .ok_or_else(|| anyhow::anyhow!("Failed to build image buffer"))?;
      img.save(&path)?;
      Ok(path)
  }
  ```
- [x] Add `frame_id: u64 = 0` counter before the frame loop; increment on each trigger
- [x] **Verify BGRA channel order** — check `capture.rs` pixel format constant; if format is already RGBA, skip the `pixel.swap(0, 2)` step

**Verification:**

- [ ] Trigger a debounce (stop scrolling for 300ms)
- [ ] Confirm `/tmp/contextura-frame-0.png` appears
- [ ] Open it — should be a legible screenshot of your screen

---

### Step 2 — Instantiate Motion Detection

**Problem:** `MotionDetector` and `DebounceStateMachine` in `motion.rs` are never created in `lib.rs`.

**Fix:**

- [x] Add imports to `lib.rs`:
  ```rust
  use crate::motion::{MotionDetector, DebounceStateMachine, DebounceEvent};
  ```
- [x] Before the frame receive loop, instantiate:
  ```rust
  let mut motion_detector = MotionDetector::new(
      settings.motion_threshold,
      settings.pixel_diff_threshold,
      settings.edge_inset_percent,
  );
  let mut debounce = DebounceStateMachine::new(
      std::time::Duration::from_millis(settings.debounce_ms as u64)
  );
  ```
- [x] Replace the current loop body with:
  ```rust
  let frame = frame_rx.recv()?;
  let motion_ratio = motion_detector.process(&frame);
  match debounce.update(motion_ratio) {
      DebounceEvent::MotionDetected => { /* Step 4 */ }
      DebounceEvent::Triggered => { /* Steps 1, 3, 4 */ }
      DebounceEvent::NoChange => continue,
  }
  ```

**Verification:**

- [ ] Scroll a page → check logs for "SCROLLING" state
- [ ] Stop scrolling → check logs for "TRIGGERED" ~300ms later
- [ ] Blinking cursor should NOT produce "TRIGGERED"

---

### Step 3 — Wire OCR and Translation Into Trigger Branch

**Problem:** OCR uses wrong path; translation results are logged but not used.

**Fix in `DebounceEvent::Triggered` branch:**

- [x] Call `save_frame_as_png()` (Step 1)
- [x] Call `ocr_engine.recognize(&png_path)` with the real path
- [x] Call `std::fs::remove_file(&png_path)` after OCR returns (whether success or error)
- [x] Skip if `ocr_results.is_empty()`
- [x] Call `translation_client.translate_batch(&texts, memory.as_context_slice())`
- [x] Use results in Step 4 (not just log them)

**Verification:**

- [ ] Open a Japanese webpage, stop scrolling
- [ ] Check logs: should see real Japanese text extracted by OCR
- [ ] Check logs: should see real English translations returned
- [ ] Check `/tmp/` — PNG should appear then disappear (cleanup working)

---

### Step 4 — Wire Styling and Emit IPC Events

**Problem:** `StylingEngine` never called. `app_handle.emit()` never called. Frontend never receives events.

**Fix:**

- [x] Add import: `use crate::styling::StylingEngine;`
- [x] Add import: `use crate::ipc::{TranslationBox, TranslationPayload};`
- [x] Add import: `use rayon::prelude::*;`
- [x] Instantiate `StylingEngine` ONCE before the loop: `let styling_engine = StylingEngine::new();`
- [x] In `DebounceEvent::MotionDetected` branch:
  ```rust
  let _ = app_handle.emit("translation-clear", ());
  ```
- [x] Before `translate_batch()` call:
  ```rust
  let _ = app_handle.emit("translation-started", serde_json::json!({"display_id": 0}));
  ```
- [x] After translations are returned, build boxes with Rayon:
  ```rust
  let styled_boxes: Vec<TranslationBox> = ocr_results
      .par_iter()
      .zip(translations.par_iter())
      .enumerate()
      .map(|(i, (ocr, translation))| {
          let (bg_color, fg_color) = styling_engine.style_for_box(&frame, &ocr.bounding_box);
          TranslationBox {
              id: format!("{}-{}", frame_id, i),
              translated: translation.clone(),
              original: ocr.text.clone(),
              x: ocr.bounding_box.x,
              y: ocr.bounding_box.y,
              width: ocr.bounding_box.width,
              height: ocr.bounding_box.height,
              is_vertical: ocr.is_vertical,
              bg_color,
              fg_color,
              confidence: ocr.confidence,
          }
      })
      .collect();
  ```
- [x] Build payload and emit:
  ```rust
  let payload = TranslationPayload {
      boxes: styled_boxes,
      scale_factor: 2.0,  // TODO: query real scale_factor from display
      display_id: 0,
      frame_id,
  };
  let _ = app_handle.emit("translation-update", &payload);
  ```
- [x] Push results to memory:
  ```rust
  for (ocr, translation) in ocr_results.iter().zip(translations.iter()) {
      translation_memory.push(ocr.text.clone(), translation.clone());
  }
  ```
- [x] Remove `#[allow(dead_code)]` from `ipc.rs`

**Verification:**

- [ ] Open a Japanese webpage, stop scrolling
- [ ] **Translation boxes should appear over the Japanese text in the overlay**
- [ ] This is the first working translation milestone

---

### Step 5 — Wire Context Invalidation

**Problem:** App switch detected but `memory.clear()` not called.

**Fix — drain `invalidation_rx` before OCR in the `Triggered` branch:**

- [x] Add to `Triggered` branch before OCR call:
  ```rust
  while let Ok(reason) = invalidation_rx.try_recv() {
      match reason {
          InvalidationReason::AppSwitch { from, to } => {
              log::info!("[Context] {} -> {} — clearing memory", from, to);
              translation_memory.clear();
              let _ = app_handle.emit("translation-clear", ());
          }
          InvalidationReason::ManualReset => {
              translation_memory.clear();
              // Do NOT emit translation-clear; user may still be reading
          }
          InvalidationReason::ModelSwitch => {
              translation_memory.clear();
              let _ = app_handle.emit("translation-clear", ());
          }
      }
  }
  ```

**Verification:**

- [ ] Translate Japanese content in Safari
- [ ] Switch to Terminal → overlay should clear; translation_memory.len() = 0
- [ ] Switch between Safari tabs → overlay should NOT clear (same bundle ID)
- [ ] Press `Cmd+Shift+M` → memory clears; overlay stays visible

---

### Step 6 — Functional Hotkeys

- [x] **`Cmd+Shift+T` (toggle overlay):**
- [x] **`Cmd+Shift+R` (force OCR):**
  - Add `force_trigger_tx: Sender<()>` / `force_trigger_rx: Receiver<()>` channel
  - In hotkey handler: `let _ = force_trigger_tx.send(());`
  - In frame loop, at top of loop body before motion check:
    ```rust
    let force = force_trigger_rx.try_recv().is_ok();
    ```
  - If `force`, skip to OCR immediately (bypass debounce)

---

### Step 7 — Stability Cleanup

After steps 1–6 produce working translations:

- [x] **Temp PNG panic cleanup** — `std::panic::set_hook` to delete `/tmp/contextura-frame-*.png`
- [ ] **Real scale_factor** — query actual `backingScaleFactor` from display info instead of hardcoded `2.0`
- [x] **Watchdog thread** — background thread polling `GET /health` every 5s; restart sidecar after 3 failures; emit `"translation-error"`
- [x] **Battery check** — implement real `IOPSCopyPowerSourcesInfo` (pmset) in `thermal.rs`
- [ ] **Sentry** — call `sentry::init()` conditionally in `lib.rs` setup

**✅ Phase P.Complete Milestone:** Japanese text on screen → stop scrolling → translation boxes appear → switch app → overlay clears → `Cmd+Shift+T` toggles → `Cmd+Shift+R` forces immediate retranslation.

---

## Phases 0–6 Status (Reference)

| Phase                           | Status                                                 |
| ------------------------------- | ------------------------------------------------------ |
| Phase 0 — Setup, CLI, settings  | ✅ Done (CLI outputs are stubs)                        |
| Phase 1 — SCKit + motion        | ✅ SCKit working; motion code exists but not wired     |
| Phase 2 — OCR + furigana        | ✅ Working when PNG path is valid                      |
| Phase 3 — Translation + context | ✅ HTTP client working; model was wrong arch           |
| Phase 4 — Styling               | ✅ Done (not called from pipeline)                     |
| Phase 5 — IPC + frontend        | ✅ Done (emit() never called from pipeline)            |
| Phase 6 — Hotkeys, tray, wizard | ⚠️ Structure done; most hotkeys stubs; wizard 1 screen |

---

## Phase 7 — Performance, E2E Testing & Hardening

**Begin after Phase P.Complete works end-to-end.**

- [ ] Profile with Xcode Instruments: Time Profiler + Allocations
- [ ] Measure end-to-end latency target: < 2s for Standard tier
- [ ] Curate `test-corpus/`: 10 Japanese PNGs (horizontal, vertical, furigana)
- [ ] Write `.expected.json` per PNG
- [ ] Make `--debug-cli` run real pipeline and print `TranslationPayload` JSON
- [ ] Make `--test-suite` call real OCR + translation (not mock)
- [ ] Set up GitHub Actions CI: run `--test-suite` on every commit
- [ ] Add `excludedWindows` in `capture.rs` to exclude overlay from SCKit
- [ ] Handle system sleep/wake: restart SCKit streams
- [ ] Handle OCR subprocess crash gracefully (already handled; verify)
- [ ] 30-minute memory leak check in Activity Monitor

---

## Phase 8 — Build, Sign & Distribution

**Begin after Phase 7.**

- [ ] Codesign `llama-server` binary independently
- [ ] Codesign `vision-helper` binary independently
- [ ] Configure Tauri signing with Apple Developer certificate
- [ ] Set bundle identifier: `com.yourname.contextura`
- [ ] Set up `xcrun notarytool` notarization
- [ ] Implement wizard screens 2–4 (model selection, download, privacy)
- [ ] RAM gate: `sysctl hw.memsize`; disable Quality Mode if < 12GB
- [ ] Populate auto-updater pubkey in `tauri.conf.json`
- [ ] Create `.dmg` via Tauri bundler
- [ ] Test on clean macOS 13 and macOS 14

---

## Backlog / v1.1+

- [ ] Multi-display support
- [ ] Gemma 4 E4B Quality Mode + `switch_model()` + real `Cmd+Shift+G`
- [ ] Apple Foundation Models native tier (macOS 26+)
- [ ] Full settings UI
- [ ] Tab-level context isolation
- [ ] Furigana tooltip on hover
- [ ] Persist translation memory across sessions
- [ ] Chinese and Korean support

---

## Quick Reference: File Status

```
lib.rs               ← THE WORK — wire all subsystems here
capture.rs           ✅ Real SCKit frames
motion.rs            ✅ Code correct — NOT wired in lib.rs
ocr.rs               ✅ Real vision-helper subprocess
translation.rs       ✅ Real HTTP — needs --jinja arg + /no_think prompt
context.rs           ✅ NSWorkspace polling — memory.clear() not wired
thermal.rs           ⚠️ Thermal real; battery hardcoded false
styling.rs           ✅ WCAG math correct — NOT called from lib.rs
ipc.rs               ✅ Structs defined — emit() never called
hotkeys.rs           ⚠️ T, R, G are log stubs
capabilities/
  default.json       ❌ Missing shell:allow-execute — FIX FIRST
models/
  nllb-*.gguf        ❌ Wrong architecture — delete or ignore
  qwen3-0.6b-*.gguf  ⬜ Download this instead
```
