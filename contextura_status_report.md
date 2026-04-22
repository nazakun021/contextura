# Contextura — Comprehensive Repository Status Report

**Generated:** 2026-04-22  
**Purpose:** Full codebase audit & cross-reference of SPEC.md / TODO.md claims vs actual implementation  
**For:** Handoff to Claude on Cloud

---

## Executive Summary

Contextura is a Tauri v2 macOS desktop app for real-time Japanese→English screen translation. The project has **strong scaffolding** across all modules, but there is a **significant gap between what the documentation claims is done and what the code actually does**. The docs (SPEC.md, TODO.md) mark Phase P (Pipeline Activation) as "✅ Done," but the code reveals the pipeline is **partially wired with critical gaps**. Many hotkey/tray actions are **log-only stubs**, the wizard is **a single screen** (not the specified 4-screen flow), and several modules have **dead code** that is never called.

### Overall Health Score: **5.5 / 10**

| Area | Score | Notes |
|------|-------|-------|
| Project Structure | 9/10 | Clean module separation, good Cargo.toml |
| Rust Backend Core | 6/10 | Modules exist but many are disconnected |
| Pipeline Integration | 4/10 | Partially wired; key steps missing |
| Frontend | 7/10 | Functional overlay rendering, correct events |
| Documentation Accuracy | 4/10 | Many items marked `[x]` are stubs or incomplete |
| Build & Distribution | 3/10 | Binaries present but Phase 8 not started |

---

## 1. File Inventory

### Rust Backend (`src-tauri/src/`)

| File | Lines | Purpose | Status |
|------|-------|---------|--------|
| [lib.rs](file:///Users/infinite/Programming/contextura/src-tauri/src/lib.rs) | 286 | Main setup + pipeline orchestration | Partially wired |
| [capture.rs](file:///Users/infinite/Programming/contextura/src-tauri/src/capture.rs) | 94 | ScreenCaptureKit bindings | ✅ Real SCKit integration |
| [motion.rs](file:///Users/infinite/Programming/contextura/src-tauri/src/motion.rs) | 271 | Motion detection + debounce | ⚠️ Code exists but **never called** from pipeline |
| [ocr.rs](file:///Users/infinite/Programming/contextura/src-tauri/src/ocr.rs) | 214 | Vision helper subprocess wrapper | ✅ Real subprocess call |
| [translation.rs](file:///Users/infinite/Programming/contextura/src-tauri/src/translation.rs) | 175 | llama-server HTTP client | ✅ Real HTTP implementation |
| [context.rs](file:///Users/infinite/Programming/contextura/src-tauri/src/context.rs) | 60 | App window tracker | ✅ Real NSWorkspace polling |
| [thermal.rs](file:///Users/infinite/Programming/contextura/src-tauri/src/thermal.rs) | 62 | Thermal/battery monitor | ⚠️ Thermal real; battery **hardcoded `false`** |
| [styling.rs](file:///Users/infinite/Programming/contextura/src-tauri/src/styling.rs) | 112 | WCAG contrast calculation | ⚠️ Code correct but **never called** |
| [ipc.rs](file:///Users/infinite/Programming/contextura/src-tauri/src/ipc.rs) | 47 | Payload structs | ⚠️ Structs defined but **never constructed** |
| [hotkeys.rs](file:///Users/infinite/Programming/contextura/src-tauri/src/hotkeys.rs) | 51 | Global shortcuts | ⚠️ Registered but most are **log-only stubs** |
| [tray.rs](file:///Users/infinite/Programming/contextura/src-tauri/src/tray.rs) | 92 | System tray menu | ⚠️ Menu built; most handlers are **log-only** |
| [settings.rs](file:///Users/infinite/Programming/contextura/src-tauri/src/settings.rs) | 86 | Settings JSON | ✅ Working |
| [downloader.rs](file:///Users/infinite/Programming/contextura/src-tauri/src/downloader.rs) | 37 | Model downloader | ⚠️ Code exists but **never called**; `#[allow(dead_code)]` |
| [cli.rs](file:///Users/infinite/Programming/contextura/src-tauri/src/cli.rs) | 38 | CLI argument parsing | ✅ Working |

### Frontend (`src/`)

| File | Lines | Purpose | Status |
|------|-------|---------|--------|
| [index.html](file:///Users/infinite/Programming/contextura/src/index.html) | 25 | Overlay page | ✅ Working |
| [overlay.js](file:///Users/infinite/Programming/contextura/src/overlay.js) | 74 | Event listeners + DOM rendering | ✅ Working |
| [overlay.css](file:///Users/infinite/Programming/contextura/src/overlay.css) | 95 | Transparent overlay styles | ✅ Working |
| [wizard.html](file:///Users/infinite/Programming/contextura/src/wizard.html) | 81 | First-run wizard | ❌ **Only 1 screen** (spec says 4) |
| [help.html](file:///Users/infinite/Programming/contextura/src/help.html) | 29 | Help page | ✅ Working (minimal) |

### Sidecars & Binaries (`src-tauri/binaries/`)

| Binary | Present | Notes |
|--------|---------|-------|
| `llama-server-aarch64-apple-darwin` | ✅ 9MB | Pre-compiled, correct target triple |
| `vision-helper-aarch64-apple-darwin` | ✅ 98KB | Compiled Swift binary |
| `vision-helper` (source) | ✅ | Swift source in `src/bin/vision-helper.swift` |
| `libggml*.dylib` (7 libs) | ✅ | llama.cpp runtime dependencies |
| `libllama*.dylib` (3 libs) | ✅ | llama.cpp core library |

---

## 2. SPEC.md Claims vs Reality — Phase-by-Phase Audit

### Phase 0 — Environment & Setup (Claimed: ✅ Done)

| SPEC Claim | Actual Status | Verdict |
|------------|---------------|---------|
| Tauri v2 project created | ✅ `tauri.conf.json` present, compiles | ✅ TRUE |
| Transparent, borderless, alwaysOnTop | ✅ Config correct | ✅ TRUE |
| Entitlements for screen-capture | ✅ `entitlements.plist` has `com.apple.security.screen-capture` | ✅ TRUE |
| All dependencies in Cargo.toml | ✅ All listed deps present | ✅ TRUE |
| `--debug-cli` flag | ⚠️ Flag exists but outputs **nothing useful** — just prints "Running in headless debug mode" then parks thread | ⚠️ STUB |
| `--list-models` | ⚠️ Prints active model name only, no manifest table | ⚠️ PARTIAL |
| `--prune-models` | ❌ Hardcoded "No unused models found" — no real scanning | ❌ STUB |
| `--test-suite` | ❌ Iterates PNGs but prints "OK (Mocked result)" — no real testing | ❌ STUB |
| `settings.json` created on first run | ✅ Real implementation | ✅ TRUE |

### Phase 1 — Screen Capture (Claimed: 🔨 Scaffolded → SPEC says ✅ Done via Phase P)

| SPEC Claim | Actual Status | Verdict |
|------------|---------------|---------|
| SCKit capture via `screencapturekit` crate | ✅ Real `SCStream` + `OutputHandler` in `capture.rs` | ✅ TRUE |
| BGRA8 pixel format, frame callback | ✅ Uses `image_buffer()` + `lock_read_only()` | ✅ TRUE |
| Bounded crossbeam channel (capacity 2) | ✅ `bounded::<CaptureFrame>(2)` | ✅ TRUE |
| Multi-display enumeration | ⚠️ Code finds display by ID or falls back to first — only captures **1 display** in `lib.rs` | ⚠️ PARTIAL |
| Exclude overlay window from capture | ❌ `excludedWindows` **not implemented** in the filter | ❌ MISSING |
| Display hot-plug handling | ❌ `CGDisplayRegisterReconfigurationCallback` **never subscribed** | ❌ MISSING |

### Phase 2 — OCR (Claimed: 🔨 Scaffolded → SPEC says ✅ Done via Phase P)

| SPEC Claim | Actual Status | Verdict |
|------------|---------------|---------|
| `vision-helper` Swift binary built | ✅ Source + compiled binary present | ✅ TRUE |
| `OcrEngine.recognize()` calls subprocess | ✅ Real `Command::new()` invocation | ✅ TRUE |
| Coordinate conversion (bottom-left → top-left) | ✅ Correct Y-axis flip in `process_vision_results()` | ✅ TRUE |
| Vertical text detection (`text_angle > π/4`) | ✅ Implemented | ✅ TRUE |
| Furigana suppression | ✅ Proximity-based algorithm implemented | ✅ TRUE |
| Confidence filter < 0.4 | ✅ Implemented | ✅ TRUE |
| CJK character filter | ✅ `contains_cjk()` checks Hiragana/Katakana/Kanji | ✅ TRUE |
| IoU merge (> 0.3) | ✅ `calculate_iou()` + merge loop | ✅ TRUE |
| Temp PNG cleanup after OCR | ❌ PNG **never deleted** in pipeline | ❌ MISSING |

### Phase 3 — Translation Engine (Claimed: 🔨 Scaffolded → SPEC says ✅ Done via Phase P)

| SPEC Claim | Actual Status | Verdict |
|------------|---------------|---------|
| `llama-server` bundled as Tauri sidecar | ✅ Binary present + `externalBin` in config | ✅ TRUE |
| `start_sidecar()` via Tauri shell | ✅ `app.shell().sidecar("llama-server")` with correct args | ✅ TRUE |
| Health check polling (`/health`) | ✅ `wait_for_ready()` polls every 500ms, 30 attempts | ✅ TRUE |
| `translate_batch()` via `/v1/chat/completions` | ✅ Real HTTP POST with correct payload format | ✅ TRUE |
| Batched numbered prompt with context | ✅ Context header + numbered format | ✅ TRUE |
| Sub-batch at 15 strings | ✅ `strings.chunks(15)` | ✅ TRUE |
| `TranslationMemory` (push, clear, context_slice) | ✅ All methods implemented | ✅ TRUE |
| Watchdog (poll /health every 5s, restart after 3 failures) | ❌ **Not implemented** — no background health polling thread | ❌ MISSING |
| `switch_model()` restarts sidecar | ❌ **Not implemented** — hotkey logs only | ❌ MISSING |
| RAM gate (Quality Mode disabled < 12GB) | ❌ **Not implemented** — no `sysctl hw.memsize` check anywhere | ❌ MISSING |
| Sidecar killed on app quit | ⚠️ `process::exit(0)` in quit — sidecar child handle **dropped but not explicitly killed** | ⚠️ IMPLICIT |

### Phase 4 — Dynamic Styling (Claimed: ✅ Done)

| SPEC Claim | Actual Status | Verdict |
|------------|---------------|---------|
| `sample_rect_ring()` — outer 2px sampling | ✅ Correct algorithm in `styling.rs` | ✅ TRUE |
| `relative_luminance()` + `linearize_channel()` | ✅ WCAG 2.1 formula correct | ✅ TRUE |
| Contrast threshold L > 0.179 | ✅ Correct | ✅ TRUE |
| Unit tests pass | ✅ 2 tests (black/white) | ✅ TRUE |
| **Actually used in pipeline** | ❌ `StylingEngine` is **never called** from `lib.rs` — compiler warns "never constructed" | ❌ DEAD CODE |

### Phase 5 — IPC & Frontend (Claimed: ✅ Done)

| SPEC Claim | Actual Status | Verdict |
|------------|---------------|---------|
| `TranslationBox` + `TranslationPayload` structs | ✅ Defined in `ipc.rs` | ✅ TRUE |
| All 4 Tauri events emitted | ❌ **None are emitted** in the pipeline — `lib.rs` never calls `emit()` | ❌ MISSING |
| Frontend listens to all 4 events | ✅ `overlay.js` has all 4 listeners | ✅ TRUE |
| Spinner on `translation-started` | ✅ CSS animation + JS handler | ✅ TRUE |
| Error banner on `translation-error` | ✅ 4s auto-dismiss | ✅ TRUE |
| Vertical text `writing-mode: vertical-rl` | ✅ CSS class present | ✅ TRUE |
| `display_id` routing to correct window | ❌ No routing logic — only 1 window | ❌ MISSING |

### Phase 6 — Hotkeys & Polish (Claimed: ✅ Done)

| SPEC Claim | Actual Status | Verdict |
|------------|---------------|---------|
| `Cmd+Shift+T` toggle overlay | ⚠️ Registered — **logs only**, no `show()`/`hide()` call | ❌ STUB |
| `Cmd+Shift+Q` quit | ✅ Calls `process::exit(0)` | ✅ TRUE |
| `Cmd+Shift+R` force OCR | ⚠️ Registered — **logs only**, no OCR trigger | ❌ STUB |
| `Cmd+Shift+M` clear memory | ✅ Calls `window_tracker.trigger_manual_reset()` | ✅ TRUE |
| `Cmd+Shift+G` toggle model | ⚠️ Registered — **logs only**, no model switch | ❌ STUB |
| Tray menu items functional | ⚠️ All items present; most are **log-only** except Settings (opens Finder) and Help (opens window) | ⚠️ PARTIAL |
| Thermal badge on tray icon | ❌ **Not implemented** | ❌ MISSING |
| First-run 4-screen wizard | ❌ **Only 1 screen** — Screen 1 only (permission). Missing: model selection, download, privacy | ❌ INCOMPLETE |
| Wizard completion flag | ✅ `wizard_completed` in settings.json | ✅ TRUE |
| `help.html` accessible from tray | ✅ Opens help window | ✅ TRUE |
| `tauri-plugin-updater` configured | ⚠️ Plugin in `Cargo.toml` + endpoints in config — but **pubkey is empty string** | ⚠️ PARTIAL |
| `sentry-rust` initialized conditionally | ⚠️ Dependency present — but **`sentry::init()` never called anywhere** | ❌ MISSING |

### Phase P — Pipeline Orchestration (Claimed: ✅ Done in SPEC; ⬜ Current Focus in TODO)

> [!CAUTION]
> **SPEC.md line 32 says "✅ Done" for Phase P, but TODO.md line 145 says "⬜ Current Focus".** These contradict each other. The code confirms TODO.md is more accurate — Phase P is **NOT complete**.

| SPEC Claim | Actual Status | Verdict |
|------------|---------------|---------|
| Pipeline loop in `lib.rs` setup | ⚠️ Exists but with **critical gaps** (see below) | ⚠️ PARTIAL |
| Start sidecar → wait for health | ✅ Implemented in the loop | ✅ TRUE |
| Load settings, init memory | ✅ Done | ✅ TRUE |
| Subscribe NSWorkspace notification | ✅ `window_tracker.start_polling()` | ✅ TRUE |
| Subscribe IOKit thermal | ⚠️ `thermal_monitor.update()` called — but **only in outer retry loop**, not in frame processing | ⚠️ PARTIAL |
| Create SCStream per display | ⚠️ Only captures **display 0** | ⚠️ PARTIAL |
| Motion detection in frame loop | ❌ **`MotionDetector` never instantiated** in the pipeline | ❌ MISSING |
| Debounce state machine in frame loop | ❌ **`DebounceStateMachine` never instantiated** | ❌ MISSING |
| Save frame as PNG on trigger | ❌ Frame is **never saved as PNG** — `temp_path` is hardcoded but no write occurs | ❌ MISSING |
| Invoke OCR on frame | ⚠️ `ocr_engine.recognize()` is called but with a **non-existent file path** (`/tmp/contextura-frame-latest.png` never written) | ❌ BROKEN |
| Emit `translation-started` | ❌ Never emitted | ❌ MISSING |
| Emit `translation-update` with styled payload | ❌ Never emitted — translations are fetched but **never sent to frontend** | ❌ MISSING |
| Emit `translation-clear` on motion | ❌ No motion detection → no clear events | ❌ MISSING |
| Build styled boxes with Rayon | ❌ Rayon never used; styling never called | ❌ MISSING |

---

## 3. Compiler Warnings (13 total)

All 13 warnings are `dead_code` warnings, confirming that significant code paths are never reached:

| Module | Warning | Implication |
|--------|---------|-------------|
| `capture.rs` | `PixelBuffer.data` never read | Frame data captured but not processed |
| `capture.rs` | `CaptureFrame.display_id` never read | Display routing not implemented |
| `motion.rs` | `DebounceState`, `DebounceEvent`, `DebounceStateMachine` never used | Motion detection **completely disconnected** |
| `motion.rs` | `MotionDetector` + all methods never used | " |
| `styling.rs` | `Rgba`, `StylingEngine` + all methods never used | Styling **completely disconnected** |
| `translation.rs` | `TranslationMemory::clear()` never used | Memory clearing from invalidation never wired |

---

## 4. Critical Pipeline Gap Analysis

The pipeline in `lib.rs` (lines 125–223) has this actual flow:

```
1. ✅ Start sidecar → wait for health
2. ✅ Start capture on display 0
3. ❌ Receive frame (but NO motion detection)
4. ❌ OCR called with non-existent PNG file
5. ⚠️ If OCR somehow succeeds, translations are fetched
6. ❌ Results are logged but NEVER sent to frontend
7. ❌ No styling applied
8. ❌ No events emitted
```

**What's missing to close the pipeline:**

1. **Motion Detection** — Instantiate `MotionDetector` + `DebounceStateMachine`, feed frames through them
2. **PNG Snapshot** — Actually write the frame buffer to disk as PNG using the `image` crate
3. **Styling** — Call `StylingEngine::sample_rect_ring()` + `get_fg_color()` for each box
4. **IPC Emission** — Build `TranslationPayload`, emit `translation-update`/`clear`/`started` events
5. **Watchdog Thread** — Background health poll + auto-restart
6. **Multi-Display** — Loop over all displays, not just display 0

---

## 5. TODO.md Accuracy Check

### Items Marked `[x]` That Are Actually Incomplete/Stubs

| TODO Item | Line | Reality |
|-----------|------|---------|
| `--debug-cli` outputs useful data | 54 | ❌ Prints static text, parks thread |
| `--test-suite` | 56 | ❌ Prints "OK (Mocked result)" for all |
| `--list-models`, `--prune-models` | 57 | ❌ Hardcoded output, no real logic |
| `switch_model()` scaffolded | 101 | ❌ No `switch_model()` function exists |
| Watchdog thread structure | 102 | ❌ No watchdog thread exists |
| IOKit thermal subscription scaffolded | 103 | ⚠️ Reads state but no subscription/notification |
| "All of these are connected to a real sidecar" | 104 | ❌ False — watchdog & thermal not connected |
| Remove `#[expect(dead_code)]` from all modules | 368 | ❌ `ipc.rs` still has `#[allow(dead_code)]` |
| Emit `translation-started/update/clear/error` | 124 | ❌ None emitted from pipeline |
| `display_id` routing to correct window | 125 | ❌ Only 1 window, no routing |
| Thermal badge on tray icon | 136 | ❌ Not implemented |
| First-run 4-screen wizard | 137 | ❌ Only 1 screen |
| `sentry-rust` initialized if opted in | 141 | ❌ Never initialized |
| P.1.3 — Wire to Motion Detector | 195-197 | ❌ Motion detector never instantiated in pipeline |
| P.3.5 — Wire Context Invalidation | 348-352 | ⚠️ Invalidation channel exists but memory clear is not called |
| P.3.6 — Wire Gemma 4 Model Switch | 354-358 | ❌ No model switch logic |
| P.4 — Full orchestration | 369-400 | ❌ Significantly incomplete |

### Items Correctly Marked `[ ]` (Not Started)

These are honestly marked as not done and remain accurate:
- P.1.2 Swift capture helper fallback
- P.2.3 tests (debug-cli with real webpage, bounding box alignment)
- P.5 smoke tests (all)
- Phase 7 performance, E2E, edge cases, memory leak
- Phase 8 code signing, notarization, packaging
- All backlog items

---

## 6. Runtime Behavior

Based on the build output and code analysis:

1. **App launches** → transparent overlay window appears (if wizard completed)
2. **Sidecar starts** → attempts to launch `llama-server` with model file
3. **Health check fails** → `"Llama-server health check timed out"` error logged repeatedly (model file likely not present at expected path)
4. **No translation occurs** → even if sidecar were healthy, the pipeline doesn't produce visible output because events are never emitted
5. **13 dead code warnings** → confirm major code paths are disconnected

---

## 7. Dependency & Config Issues

| Issue | Severity | Details |
|-------|----------|---------|
| Updater pubkey empty | Medium | `tauri.conf.json` has `"pubkey": ""` — updater won't work |
| Capabilities missing shell permission | High | `default.json` doesn't include `shell:allow-execute` or sidecar permissions — sidecar spawn may fail |
| `edition = "2024"` in Cargo.toml | Low | Using Rust 2024 edition — ensure toolchain supports it |
| `image` crate version `0.24` | Low | Listed in deps but never imported/used in pipeline code |
| Battery check hardcoded false | Medium | `thermal.rs:46` — `check_on_battery()` always returns `false` |

---

## 8. Summary: What Actually Works End-to-End

✅ **Fully Working:**
- App compiles and launches
- Transparent click-through overlay window
- Settings JSON load/save with defaults
- First-run wizard (1 screen only) with completion flag
- System tray with all menu items (most are log-only)
- `Cmd+Shift+Q` quit, `Cmd+Shift+M` manual reset
- Help page opens from tray
- Settings folder opens from tray
- Frontend event listeners ready for all 4 event types
- ScreenCaptureKit frame capture (real frames)
- Vision-helper Swift binary (compiles and runs)
- llama-server sidecar launch + health check
- Translation HTTP client with batching + context memory

❌ **Not Working End-to-End:**
- The complete capture→motion→OCR→translate→render pipeline
- No visible translations ever appear in the overlay
- Motion detection (code exists, never called)
- Dynamic styling (code exists, never called)
- IPC event emission (structs exist, never sent)
- Model switching
- Watchdog crash recovery
- Multi-display support
- 4-screen onboarding wizard
- Sentry crash reporting
- Auto-updater (empty pubkey)

---

## 9. Recommendations for Next Steps

> [!IMPORTANT]
> **Priority 1 — Close the pipeline gap.** The individual subsystems (capture, OCR, translation, styling) all work in isolation. The missing piece is wiring them together in `lib.rs` with proper motion detection, PNG snapshot, styling, and IPC emission.

1. **Wire Motion Detection** — Instantiate `MotionDetector` + `DebounceStateMachine` in the frame loop
2. **Write PNG Snapshots** — Use `image` crate to save frame buffer on debounce trigger
3. **Wire Styling** — Call `StylingEngine` on each OCR result before building payload
4. **Emit IPC Events** — Construct `TranslationPayload` and call `app_handle.emit()`
5. **Add Watchdog** — Background thread polling `/health` every 5s
6. **Fix Capabilities** — Add `shell:allow-execute` or sidecar-specific permissions
7. **Complete Wizard** — Add screens 2-4 (model selection, download, privacy)
8. **Make Hotkeys Functional** — Toggle overlay, force OCR, model switch
9. **Fix Documentation** — Reconcile SPEC.md "Phase P ✅ Done" with reality
