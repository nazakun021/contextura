# Roadmap

The implementation path for Contextura is split into concrete phases, moving from the initial integration to a production-ready, highly optimized experience.

## Phase 1: Core Pipeline Hardening (Current Focus)
Ensure the basic capture-OCR-translation loop is robust, fast, and bug-free.

* **Live TranslateGemma Verification**: Get `TranslateGemma` working properly as the primary, high-quality translation model before extending support to other models (e.g., Qwen family).
* **Test Corpus Solidification**: Replace placeholder files in `test-corpus/` with actual OCR/translation fixtures.
* **Debounce & Hotkeys Tuning**: Adjust screen settlement detection to avoid triggering OCR during rapid scrolling.
* **App Invalidation**: Verify app switching successfully invalidates overlays and clears the translation memory context.

## Phase 2: Production-Grade Runtime
Optimize resource usage, stream reliability, and sidecar lifecycles to make the application daily-driver ready.

* **Sticky Capture Stream**: Create `SCStream` once at startup and modify configuration dynamically (via `update_configuration()`) instead of destroying/re-creating the stream on every frame.
* **Long-Lived LLM Sidecar**: Keep the `llama-server` process alive throughout the session. Use the `/v1/chat/completions` or cache reset API instead of restarting the sidecar when memory needs clearing.
* **App Updater Setup**: Add code signing public keys to `tauri.conf.json` for secure updates.
* **Model Downloader UI**: Build a safe UI flow to fetch GGUF models directly from Hugging Face into the application support folder.
* **Telemetry Permission Consent**: Add a wizard prompt requesting opt-in permission before enabling Sentry/telemetry.

## Phase 3: Advanced Features
Scale the application to multi-monitor environments and prepare for a public beta.

* **Concurrent Multi-Display Support**: Capture all connected displays concurrently and overlay translated text frames on their respective screens.
* **Memory Tiers & Quality Gates**: Introduce RAM gating policies for systems with low memory (e.g. 8GB Macs).
* **Extended Model Manifests**: Support Qwen-style decoder GGUF models as lightweight alternatives for systems under heavy resource constraints.
