# Roadmap

The implementation path for Contextura is split into concrete phases, moving from the initial integration to a production-ready, highly optimized experience.

## Phase 1: Core Pipeline Hardening (Completed)

Ensure the basic capture-OCR-translation loop is robust, fast, and bug-free.

- **Live TranslateGemma Verification** [✅]: TranslateGemma is verified and supported as the primary model.
- **Test Corpus Solidification** [✅]: Golden tests and `test-corpus/` integration assertions are live.
- **Debounce & Hotkeys Tuning** [✅]: Screen settlement and hotkeys are tuned.
- **App Invalidation** [✅]: App switching successfully invalidates overlays and clears the translation memory context.

## Phase 2: Production-Grade Runtime (Completed)

Optimize resource usage, stream reliability, and sidecar lifecycles to make the application daily-driver ready.

- **Sticky Capture Stream** [✅]: Persistent `SCStream` created once and configured dynamically.
- **Long-Lived LLM Sidecar** [✅]: Keep the `llama-server` process alive throughout the session.
- **App Updater Setup** [✅]: Tauri updater plugin and configuration are set up.
- **Model Downloader UI** [🟡]: Downloader helper exists and wizard screens 1–4 are implemented; full in-app download workflow wiring is still pending.
- **Telemetry Permission Consent** [✅]: Telemetry control / Sentry configured via optional env vars.

## Phase 3: Advanced Features (Completed)

Scale the application and prepare for a public beta.

- **Single-Display Capture Hardening** [✅]: Display targeting is robust and dynamic for the current single-display runtime contract.
- **Memory Tiers & Quality Gates** [✅]: Quality/Standard/Custom model tiers classification integrated in manifest loading.
- **Extended Model Manifests** [✅]: Qwen-style decoder GGUF models are supported as lightweight alternatives.
- **In-Memory OCR Handoff** [✅]: Runtime OCR now streams PNG bytes to `vision-helper --stdin`, removing per-frame file-path handoff in the hot path.
- **Latency Tracepoints** [✅]: `[Latency]` debug logs now cover OCR stage, concurrent styling+translation stage, and chat completion request timing.

## Phase 4: Release Hardening (In Progress)

Close remaining production-readiness gaps.

- **Manual End-to-End Smoke Verification** [🟡]: Required live pass with a valid local model and real Japanese content.
- **Automated Wire-To-Wire Smoke Harness** [✅]: `scripts/smoke-wire-to-wire.sh` now validates compile/test gates plus OCR→translation CLI probes against `test-corpus`.
- **Updater Signing Public Key** [🟡]: Configure `plugins.updater.pubkey` before production release.
- **Frontend CSP Hardening** [🟡]: Replace null CSP with restrictive policy for packaged app windows.
