# TODO.md — Contextura

**Stack:** Rust · Tauri v2 · ScreenCaptureKit · Swift `vision-helper` · `llama-server`  
**Platform:** macOS 13+ · Apple Silicon  
**Last Updated:** 2026-04-26

## Manual Things You Still Need To Do

### Required setup

- [ ] Download `translategemma-4b-it.Q4_K_M.gguf` into `~/Library/Application Support/contextura/models/`
- [ ] Make sure the active model in `~/Library/Application Support/contextura/settings.json` is `translategemma-4b-it.Q4_K_M` if you want to exercise the new default path
- [ ] Grant Screen Recording permission to Contextura if macOS has not already allowed it

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
