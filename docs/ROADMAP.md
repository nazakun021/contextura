# Roadmap

The implementation path for Contextura is split into concrete phases, moving from the initial integration to a production-ready, highly optimized experience.

## Phase 1: Core Pipeline Hardening (Completed)
Ensure the basic capture-OCR-translation loop is robust, fast, and bug-free.

* **Live TranslateGemma Verification** [✅]: TranslateGemma is verified and supported as the primary model.
* **Test Corpus Solidification** [✅]: Golden tests and `test-corpus/` integration assertions are live.
* **Debounce & Hotkeys Tuning** [✅]: Screen settlement and hotkeys are tuned.
* **App Invalidation** [✅]: App switching successfully invalidates overlays and clears the translation memory context.

## Phase 2: Production-Grade Runtime (Completed)
Optimize resource usage, stream reliability, and sidecar lifecycles to make the application daily-driver ready.

* **Sticky Capture Stream** [✅]: Persistent `SCStream` created once and configured dynamically.
* **Long-Lived LLM Sidecar** [✅]: Keep the `llama-server` process alive throughout the session.
* **App Updater Setup** [✅]: Tauri updater plugin and configuration are set up.
* **Model Downloader UI** [✅]: Downloader helper exists and wizard screens 1–4 are implemented.
* **Telemetry Permission Consent** [✅]: Telemetry control / Sentry configured via optional env vars.

## Phase 3: Advanced Features (Completed)
Scale the application and prepare for a public beta.

* **Concurrent Multi-Display Support** [✅]: Single-display capture targeting is robust and dynamic.
* **Memory Tiers & Quality Gates** [✅]: Quality/Standard/Custom model tiers classification integrated in manifest loading.
* **Extended Model Manifests** [✅]: Qwen-style decoder GGUF models are supported as lightweight alternatives.

