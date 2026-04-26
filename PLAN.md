# PLAN.md — Contextura: Bug Audit & Production Hardening

**Prepared:** 2026-04-26  
**Scope:** Full codebase review of all `.rs` source files and documentation.  
**Status of app:** Single-display pipeline wired; translation not reliably working in practice.

---

## Part 1 — Bug Inventory

The following issues were found through static analysis. Severity ratings:
**🔴 Critical** = breaks the feature in the normal path · **🟠 High** = reliability/correctness hazard · **🟡 Medium** = degrades quality or performance · **🔵 Minor** = cleanup / defensive coding.

---

### BUG-01 🔴 — Sidecar never restarts after `wait_for_ready` failure

**File:** `lib.rs` ~L698–L716  
**Problem:** When `wait_for_ready()` (180 s timeout) fails, `failure_count` is incremented and the outer loop `continue`s. But `sidecar_started` is **never reset to `false`**, so the sidecar is never restarted — every retry just re-polls the dead process. With the 180 s inner timeout and a 1 s sleep, it takes approximately **30 × 181 s ≈ 90 minutes** before the `failure_count > 30` guard fires and the app gives up entirely.

**Fix:**

```rust
// In the Err arm of wait_for_ready():
failure_count += 1;
sidecar_started = false;   // ← ADD THIS — forces a restart on next iteration
if failure_count > 5 {     // ← tighter threshold (see BUG-12)
    ...
}
sleep(Duration::from_secs(2)).await;
continue;
```

---

### BUG-02 🔴 — `thermal_monitor.update()` spawns `pmset` on every frame

**File:** `lib.rs` ~L833, `thermal.rs` `check_on_battery()`  
**Problem:** `thermal_monitor.update()` is called unconditionally at the bottom of the inner `'capture` loop, which runs every 10 ms (up to 100 iterations/second). Each call to `update()` spawns a **`pmset -g batt` subprocess** synchronously. Spawning a subprocess every 10 ms saturates the process table, adds ~5 ms of latency per frame, and causes measurable CPU overhead — directly contributing to the "not working properly" symptom under any real workload.

**Fix:** Throttle `thermal_monitor.update()` to at most once every 30 seconds using an `Instant` gate:

```rust
let mut last_thermal_check = Instant::now() - Duration::from_secs(31); // force first check
// ... inside 'capture loop:
if last_thermal_check.elapsed() > Duration::from_secs(30) {
    thermal_monitor.update();
    last_thermal_check = Instant::now();
}
```

---

### BUG-03 🔴 — Motion detector receives BGRA data but applies RGBA grayscale formula

**File:** `lib.rs` ~L801–L804, `motion.rs` `downsample()`  
**Problem:** `motion_detector.downsample(&frame.buffer.data, ...)` passes the raw capture buffer, which is in **BGRA** channel order (as explicitly set in `capture.rs` via `PixelFormat::BGRA`). The `downsample()` function reads bytes as `[R=0, G=1, B=2]` and applies the standard luminance formula `0.299·R + 0.587·G + 0.114·B`. With BGRA data, what it actually computes is `0.299·B + 0.587·G + 0.114·R` — the blue and red channels are swapped. This systematically skews the grayscale values, degrading motion sensitivity and causing false triggering or missed triggers depending on scene content.

**Fix:** Either pass the already-converted `rgba_data` to the motion detector, or correct the channel indexing. The cleanest solution is to note in the API that the buffer must be RGBA and use the converted buffer:

```rust
// lib.rs — swap BGRA→RGBA once, then use rgba_data for BOTH motion and OCR
let rgba_data = swap_bgra_to_rgba(&frame.buffer.data);
let thumbnail = motion_detector.downsample(&rgba_data, frame.buffer.width, frame.buffer.height);
```

Also rename the `downsample` parameter from `rgba_data` to make the expected format explicit.

---

### BUG-04 🟠 — Qwen3 numbered-batch parse silently produces empty translations

**File:** `translation.rs` ~L343–L363  
**Problem:** After the Qwen3 batch response, only lines matching `^(\d+): ` are parsed. If the model adds markdown bold (`**1:**`), uses periods (`1. text`), emits a preamble, or truncates the response (see BUG-10), the `final_results` slot stays as `String::new()`. Empty strings are then zipped with OCR boxes and emitted to the overlay, producing **blank translation boxes** with no error logged. The user sees boxes but no text.

**Fix:**

1. Broaden the line parser to handle `N.`, `**N:**`, and `N)` formats.
2. After parsing, log a `warn!` for each slot that remains empty.
3. Fall back to the raw response text if only one string was in the chunk and the entire parse fails.
4. Raise `max_tokens` (see BUG-10).

```rust
// Broader match for "1:", "1.", "**1:**", "1)"
let norm = line.trim_start_matches('*').trim_end_matches('*');
if let Some((num, text)) = norm.split_once(['.', ':', ')'].as_ref())
    && let Ok(idx) = num.trim().parse::<usize>() { ... }
```

---

### BUG-05 🟠 — `process::exit(0)` in hotkeys and tray bypasses cleanup

**Files:** `hotkeys.rs` L61, `tray.rs` L114  
**Problem:** Both Quit handlers call `std::process::exit(0)`, which does a hard POSIX exit — Rust drop handlers, Tauri cleanup, and the panic hook (which removes temp PNGs) are all **skipped**. On macOS, this also means the sidecar `llama-server` child process may be orphaned (it is not a real Tauri sidecar in the forked sense — it keeps running).

**Fix:** Use `app_handle.exit(0)` (Tauri's graceful exit) in both files. For the hotkey handler the `app_handle` is already available:

```rust
// hotkeys.rs
app_handle.exit(0);

// tray.rs — app_handle is available as the first arg to on_menu_event
app_handle.exit(0);
```

Add a `tauri::RunEvent::ExitRequested` handler in `lib.rs` to kill the sidecar child before the process ends.

---

### BUG-06 🟠 — Unwrap calls in `start_sidecar` can panic

**File:** `translation.rs` ~L227–L256  
**Problem:** Three `.unwrap()` calls that can panic in production:

```rust
app.path().resource_dir().unwrap()   // panics if resource dir not configured
binaries_dir.to_str().unwrap()       // panics on non-UTF-8 path
model_path.to_str().unwrap()         // panics on non-UTF-8 model path
```

The `start_sidecar` function already returns `anyhow::Result<()>`, so these should propagate errors.

**Fix:** Replace all three with `?`-propagating alternatives:

```rust
let resource_dir = app.path().resource_dir()
    .map_err(|e| anyhow::anyhow!("resource_dir unavailable: {e}"))?;
let binaries_dir_str = resource_dir.join("binaries")
    .to_str()
    .ok_or_else(|| anyhow::anyhow!("binaries dir path is not UTF-8"))?
    .to_string();
let model_str = model_path.to_str()
    .ok_or_else(|| anyhow::anyhow!("model path is not UTF-8: {:?}", model_path))?;
```

---

### BUG-07 🟡 — `translategemma_seed_history` caps history by chunk size

**File:** `translation.rs` ~L265–L272  
**Problem:**

```rust
let keep = history.len()
    .min(TRANSLATEGEMMA_HISTORY_LIMIT)
    .min(chunk_strings.len());  // ← wrong
```

When translating a single string (common case for brief UI labels), `chunk_strings.len() == 1`, so `keep` is capped at **1** regardless of how much history exists. This throws away all but one memory entry for every single-string translation, effectively gutting context memory for the most common case.

**Fix:** Remove the `.min(chunk_strings.len())` term entirely:

```rust
let keep = history.len().min(TRANSLATEGEMMA_HISTORY_LIMIT);
let seed = history[history.len().saturating_sub(keep)..].to_vec();
```

---

### BUG-08 🟡 — `conversation_history.remove(0)` is O(n)

**File:** `translation.rs` ~L404–L406  
**Problem:** The per-chunk conversation history in the TranslateGemma path is a `Vec<(String, String)>`. Removing from the front with `.remove(0)` is O(n) and shifts every element. While `TRANSLATEGEMMA_HISTORY_LIMIT` is currently 6 so the cost is trivial, it is incorrect data structure usage and a future footgun.

**Fix:** Use `VecDeque<(String, String)>` with `.pop_front()`:

```rust
let mut conversation_history: VecDeque<(String, String)> = /* seed */;
// ...
if conversation_history.len() > TRANSLATEGEMMA_HISTORY_LIMIT {
    conversation_history.pop_front();
}
```

---

### BUG-09 🟡 — Qwen3 `max_tokens: 512` too small for 15-string batches

**File:** `translation.rs` ~L354  
**Problem:** A batch of 15 Japanese strings, each translating to 5–15 English words plus the `N: ` prefix, requires roughly 15 × 20 = 300 tokens minimum. With the numbered format overhead and any preamble the model emits, 512 tokens is tight and will cause **silent truncation** — the last few translations in a batch simply vanish, triggering the empty-slot problem from BUG-04.

**Fix:** Increase to 1024 tokens, or compute it dynamically as `(chunk_strings.len() * 80).max(512)`:

```rust
"max_tokens": (chunk_strings.len() * 80).max(512)
```

---

### BUG-10 🟡 — `failure_count > 30` guard fires after ~90 minutes

**File:** `lib.rs` ~L698–L716  
**Problem:** Each failed `wait_for_ready()` call runs for up to 180 s (its internal timeout), then the loop sleeps 1 s. So `failure_count > 30` fires after 30 × (180 s + 1 s) ≈ **90 minutes** of waiting. The intent is clearly to give up after a short number of attempts, not 90 minutes. After applying BUG-01's fix (resetting `sidecar_started`), each failure now restarts the sidecar before retrying — making fast retries safe.

**Fix:** Reduce threshold to 5 restart attempts and tighten the ready-wait timeout for retries:

```rust
// Add a retry-phase timeout constant
const RETRY_READY_TIMEOUT: Duration = Duration::from_secs(30);

// Use shorter timeout after initial start
let ready_result = if failure_count == 0 {
    client.lock().await.wait_for_ready().await          // 180 s first attempt
} else {
    client.lock().await.wait_for_ready_retry().await    // 30 s on retries
};

if failure_count > 5 { /* give up */ }
```

Expose `wait_for_ready_with_timeout` as a pub method and add a `wait_for_ready_retry()` wrapper that calls it with `RETRY_READY_TIMEOUT`.

---

### BUG-11 🔵 — No cleanup of stale temp PNGs from prior sessions on startup

**File:** `lib.rs`, `run()` setup  
**Problem:** The panic hook cleans up `/tmp/contextura-frame-*.png` on crash, but not on normal startup. After a previous crash, stale PNGs accumulate and can be several hundred MB if the previous session ran for hours.

**Fix:** Add a one-shot cleanup at startup:

```rust
// In run(), before tauri::Builder
let _ = std::process::Command::new("sh")
    .args(["-c", "rm -f /tmp/contextura-frame-*.png"])
    .output();
```

---

### BUG-12 🔵 — `NSWorkspace` polled from a non-main thread without an autorelease pool

**File:** `context.rs` ~L29–L47  
**Problem:** The polling thread calls `NSWorkspace::sharedWorkspace()` and `frontmostApplication()` directly without an `NSAutoreleasePool`. On macOS, Objective-C objects created on non-main threads that are not in an autorelease pool context will leak, and some AppKit objects require main-thread access. In practice this usually works for read-only queries, but it is undefined behavior and can cause subtle memory leaks or crashes under load.

**Fix:** Wrap each iteration body with an autorelease pool using `objc2`'s `autoreleasepool`:

```rust
use objc2::rc::autoreleasepool;
loop {
    thread::sleep(Duration::from_millis(500));
    autoreleasepool(|_pool| {
        // NSWorkspace calls here
    });
}
```

---

## Part 2 — Hardening Plan

Beyond fixing the above bugs, the following changes are needed to make the pipeline production-ready.

---

### HARDEN-01 — Translation error recovery: per-item retry in Qwen3 batch mode

**Current state:** If the Qwen3 batch call fails (network error, HTTP error, or parse error), the entire `translate_batch` call returns `Err`, and all OCR results for that frame are discarded with a toast shown to the user.

**Change:** Add a single retry with a 500 ms delay before failing the batch. On a parse failure (some items empty), attempt individual single-string re-requests for the empty slots before giving up.

```rust
// After initial batch parse, collect empty slots
let empty_indices: Vec<usize> = final_results.iter().enumerate()
    .filter(|(_, t)| t.is_empty())
    .map(|(i, _)| i)
    .collect();

if !empty_indices.is_empty() {
    log::warn!("[Translation] {} slots empty after batch parse, retrying individually", empty_indices.len());
    for idx in empty_indices {
        if let Ok(single) = self.translate_single_qwen(&strings[idx]).await {
            final_results[idx] = single;
        }
    }
}
```

---

### HARDEN-02 — Translation health check: pre-flight before each batch

**Current state:** The pipeline starts translating immediately after the 180 s ready check. If `llama-server` becomes unhealthy mid-session (e.g., context overflow, OOM), the batch call returns an error and the user sees a toast, but there is no automatic recovery within the frame processing path.

**Change:** Before calling `translate_batch`, do a quick non-blocking health check (single GET to `/health` with a 2 s timeout). If it fails, emit a `ReloadRuntime` command and skip the current frame:

```rust
// In process_capture_frame(), before translate_batch:
if client.lock().await.quick_health_check().await.is_err() {
    log::warn!("[Translation] Pre-flight health check failed — skipping frame and requesting runtime reload");
    let _ = pipeline_tx.try_send(PipelineCommand::ReloadRuntime {
        reason: "health check failed before batch".to_string(),
    });
    return;
}
```

---

### HARDEN-03 — Graceful sidecar shutdown on app exit

**Current state:** `process::exit(0)` leaves `llama-server` as an orphaned process (BUG-05 partial fix). Even after switching to `app_handle.exit(0)`, the sidecar child is stored inside an `Arc<AsyncMutex<TranslationClient>>` and is only killed when the mutex guard is dropped, which may not happen before the process exits.

**Change:** Add a `tauri::RunEvent::Exit` handler in `run()` that explicitly kills the sidecar:

```rust
.build_and_run(tauri::generate_context!(), |app_handle, event| {
    if let tauri::RunEvent::Exit = event {
        // Signal pipeline thread to shut down cleanly
        let _ = pipeline_tx_for_exit.try_send(PipelineCommand::Shutdown);
    }
})
```

Add `PipelineCommand::Shutdown` variant that kills the sidecar child before returning from `block_on`.

---

### HARDEN-04 — Settings and model polling decoupled from capture loop

**Current state:** `Settings::load()` and `active_model_status()` are called on every outer loop iteration. While the outer loop only re-enters after the inner capture loop breaks, during error conditions (model missing, sidecar failing) the outer loop can spin at a 5 s interval with disk reads.

**Change:** Cache the loaded settings and re-read only when a `ReloadRuntime` command is received or after a fixed interval (60 s). The `active_model_id` comparison already serves as a change-detection mechanism; keep that but avoid re-reading settings every restart cycle.

---

### HARDEN-05 — OCR subprocess: use `wait_with_output` with OS-level timeout

**Current state:** `ocr.rs` polls `child.try_wait()` in a 25 ms sleep loop and manually kills the child after `OCR_HELPER_TIMEOUT` (8 s). This is correct but uses busy-polling on the main OCR thread.

**Change:** On macOS, use `std::thread::spawn` + a channel to run `child.wait_with_output()` concurrently, then `recv_timeout` on the channel. This eliminates the polling loop and is more precise:

```rust
let (tx, rx) = std::sync::mpsc::channel();
std::thread::spawn(move || {
    let _ = tx.send(child.wait_with_output());
});
match rx.recv_timeout(OCR_HELPER_TIMEOUT) {
    Ok(Ok(output)) => { /* process output */ }
    Ok(Err(e)) => anyhow::bail!("vision-helper I/O error: {e}"),
    Err(_) => anyhow::bail!("vision-helper timed out"),
}
```

---

### HARDEN-06 — TranslateGemma: add `system` prompt for consistent output

**Current state:** TranslateGemma requests have no system message, relying entirely on the model's instruction-tuning to infer the translation task from the structured user message content. In practice, without a system prompt, some model responses include explanatory text, parenthetical notes, or romanisation alongside the translation.

**Change:** Add a minimal system message before the conversation:

```rust
let mut messages = vec![json!({
    "role": "system",
    "content": "You are a Japanese-to-English translator. Output only the English translation, nothing else."
})];
messages.extend(Self::build_translategemma_conversation(history, input_text));
```

---

### HARDEN-07 — Frame deduplication: skip OCR if frame is identical to previous

**Current state:** After the debounce settles, every trigger processes the frame regardless of whether it is visually identical to the last translated frame. If the user is on a static screen, every debounce cycle re-runs OCR and translation with the same result.

**Change:** Store a fast perceptual hash (sum of thumbnail bytes as a u64 is sufficient) of the last processed frame. If the new frame hash matches, emit the cached `TranslationPayload` directly without re-running OCR or translation:

```rust
let frame_hash: u64 = thumbnail.iter().map(|&b| b as u64).sum();
if frame_hash == last_processed_hash {
    log::debug!("[Pipeline] Frame identical to last processed, skipping OCR");
    continue;
}
last_processed_hash = frame_hash;
```

---

### HARDEN-08 — Temp PNG cleanup: delete after styling, not after OCR

**Current state:** The temp PNG is deleted immediately after OCR (`let _ = std::fs::remove_file(&png_path)`), before styling happens. This is correct because `styling.rs` samples from the in-memory `rgba_data` buffer, not the file. But a crash between OCR and styling leaves no file for debugging.

**Change:** Keep this as-is (file deletion after OCR is correct since styling uses the buffer), but add a comment clarifying why, and ensure the `latest_path` copy is always written last so it survives for debugging.

---

### HARDEN-09 — Overlay IPC: debounce `translation-clear` during heavy scrolling

**Current state:** `translation-clear` is emitted on every `DebounceEvent::MotionDetected`. With fast scrolling, this fires many times per second, causing the overlay to rapidly flash clear. The flashing itself is not harmful but is visually distracting.

**Change:** Only emit `translation-clear` on the first motion event after an Idle or Triggered state, not on every Scrolling-state update:

```rust
DebounceEvent::MotionDetected => {
    if !was_scrolling {  // track bool `was_scrolling`
        let _ = app_handle.emit("translation-clear", ());
        was_scrolling = true;
    }
}
DebounceEvent::Triggered | DebounceEvent::None => {
    was_scrolling = false;
}
```

---

## Part 3 — Implementation Order

Fix the bugs in severity order. Each group is independent and can be PRed separately.

### Phase A — Critical fixes (must land before any live testing)

| #   | Change                                                                 | File(s)                 |
| --- | ---------------------------------------------------------------------- | ----------------------- |
| A1  | BUG-01: Reset `sidecar_started = false` on `wait_for_ready` failure    | `lib.rs`                |
| A2  | BUG-02: Throttle `thermal_monitor.update()` to ≤1/30s                  | `lib.rs`                |
| A3  | BUG-03: Pass `rgba_data` (converted) to `motion_detector.downsample()` | `lib.rs`                |
| A4  | BUG-04 + BUG-09: Broaden Qwen3 parser + raise max_tokens to 1024       | `translation.rs`        |
| A5  | BUG-05: Replace `process::exit(0)` with `app_handle.exit(0)`           | `hotkeys.rs`, `tray.rs` |
| A6  | BUG-06: Replace `.unwrap()` in `start_sidecar`                         | `translation.rs`        |

### Phase B — High-priority hardening (required before shipping)

| #   | Change                                                        | File(s)                    |
| --- | ------------------------------------------------------------- | -------------------------- |
| B1  | BUG-07: Fix `translategemma_seed_history` chunk-size cap      | `translation.rs`           |
| B2  | BUG-08: Replace `Vec::remove(0)` with `VecDeque::pop_front()` | `translation.rs`           |
| B3  | BUG-10: Tighten `failure_count` threshold and retry timeout   | `lib.rs`, `translation.rs` |
| B4  | HARDEN-03: Graceful sidecar shutdown on exit                  | `lib.rs`                   |
| B5  | HARDEN-06: Add system prompt for TranslateGemma               | `translation.rs`           |

### Phase C — Reliability hardening

| #   | Change                                                       | File(s)                    |
| --- | ------------------------------------------------------------ | -------------------------- |
| C1  | BUG-11: Clean stale temp PNGs on startup                     | `lib.rs`                   |
| C2  | BUG-12: Autorelease pool in context polling thread           | `context.rs`               |
| C3  | HARDEN-01: Per-item retry for empty Qwen3 slots              | `translation.rs`           |
| C4  | HARDEN-02: Pre-flight health check before each batch         | `lib.rs`, `translation.rs` |
| C5  | HARDEN-05: Replace OCR busy-poll with channel + recv_timeout | `ocr.rs`                   |

### Phase D — Polish / performance

| #   | Change                                                           | File(s)  |
| --- | ---------------------------------------------------------------- | -------- |
| D1  | HARDEN-07: Frame deduplication via thumbnail hash                | `lib.rs` |
| D2  | HARDEN-09: Debounce `translation-clear` flicker during scrolling | `lib.rs` |
| D3  | HARDEN-04: Cache settings across outer loop iterations           | `lib.rs` |
| D4  | HARDEN-08: Clarify temp PNG lifecycle comments                   | `lib.rs` |

---

## Part 4 — Verification Checklist

After all Phase A and B changes:

- [x] `cargo test --manifest-path src-tauri/Cargo.toml` passes
- [x] `cargo check --manifest-path src-tauri/Cargo.toml` passes (no new warnings)
- [ ] Launch app, wait for sidecar ready log line within 60 s
- [ ] Open Japanese text; verify translations appear within ~300 ms of settling
- [ ] Confirm `Cmd+Shift+R` force-scans the cached frame and shows translations
- [ ] Scroll continuously for 10 s; confirm overlay clears once (not on every frame) and re-populates after settling
- [ ] Kill `llama-server` manually (`pkill llama-server`); confirm watchdog restarts it within 15 s and shows a notice
- [ ] Confirm `/tmp/contextura-frame-latest.png` does NOT contain the overlay itself
- [ ] Quit via `Cmd+Shift+Q`; confirm `llama-server` process is gone (`pgrep llama-server` returns nothing)
- [ ] Check `Activity Monitor` — no orphaned `pmset` processes during normal operation
