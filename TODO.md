# TODO.md — Contextura

**Stack:** Rust · Tauri v2 · ScreenCaptureKit · Swift `vision-helper` · `llama-server`  
**Platform:** macOS 13+ · Apple Silicon  
**Last Updated:** 2026-04-26

## Manual Things You Still Need To Do

### Required setup

- [x] Download `translategemma-4b-it.Q4_K_M.gguf` into `~/Library/Application Support/contextura/models/`
- [x] Make sure the active model in `~/Library/Application Support/contextura/settings.json` is `translategemma-4b-it.Q4_K_M` if you want to exercise the new default path
- [x] Grant Screen Recording permission to Contextura if macOS has not already allowed it

### Live app verification

- [ ] Launch the app and confirm live translations appear over real Japanese text with the TranslateGemma model
- [ ] Check whether OCR-detected Japanese lines that previously produced empty overlays now translate consistently
- [ ] Verify `Cmd+Shift+R` force scan works immediately on the cached frame
- [ ] Verify `Cmd+Shift+M` really clears translation memory, not just the overlay
- [ ] Verify the overlay no longer appears inside `/tmp/contextura-frame-latest.png`
- [ ] Verify the new debounce behavior feels closer to the intended `200ms` settle time and no longer needs a second scroll gesture
- [ ] Verify switching apps clears overlay content and resets translation context as expected
- [ ] Verify tray actions still behave correctly in a live session
- [ ] Optionally simulate a sidecar failure and confirm the watchdog restart notice is visible and recovery works

### Repo assets and release prep

- [ ] Replace the placeholder `test-corpus/*.png` fixtures with real screenshots plus matching `*.expected.json` files
- [ ] Add the real updater signing public key to `src-tauri/tauri.conf.json`
- [ ] Decide whether 1.0 is explicitly single-display only, or whether you want to do multi-display work before packaging

---

What is the "Production-Grade" Way?

In a production app, you don't kill the worker; you reset its state.

1. Long-Lived LLM Sidecar
   Instead of the watchdog killing llama-server at the first sign of trouble, we keep it alive for the entire duration of the app session.

- Model Switching: This is the only time you should restart the process (since llama.cpp needs to load new weights).
- State Reset: Instead of killing the process to clear memory, we use the llama-server API to clear the KV cache or simply clear our local Rust TranslationMemory (which we already
  do).

2. Persistent Capture Stream
   Currently, your DisplayManager stops and starts the capture stream every time the loop ticks over.

- Production way: Create the SCStream once when the app starts.
- Dynamic Updates: If you need to change excluded windows (e.g., you opened the Settings window), you use stream.update_configuration() instead of stream.stop().

3. Passive Watchdog
   Instead of a watchdog that kills, use a Recovery Loop.

- If a request fails, back off for 1 second and try again.
- Only restart the process if the health check fails and the process ID (PID) is no longer found in the system.

---

Should we do it?
Yes. The "400 Bad Request" you just had was actually made worse by the restart logic—the app was trying to recover by hitting a "cold" server.

How to start the transition

I recommend we start by fixing the Capture Engine so it doesn't destroy the stream every time the screen is still.

I'll update the DisplayManager to be "Sticky"—if a stream for that display is already running, it will just return the existing receiver instead of killing the process.
