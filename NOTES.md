It’s mostly not translating because the bottleneck is OCR, not the translation model.

From your screenshot, some English lines already appear, which means the app is doing this path at least sometimes:

screen -> OCR -> text translation -> overlay

But most of the Japanese remains untouched, which usually means the Japanese text was never extracted cleanly enough from the image. In this codebase, that matches what I found
earlier: the standalone vision-helper OCR path is still unreliable on real frames, and the app only translates whatever OCR returns. If OCR misses a line, the model never sees
it.

So the short answer is:

- The current failure is primarily OCR / text-detection quality.
- The current model is not the main reason those Japanese lines are staying untranslated.
- Switching models alone will not fix missing text on screen.

About the current model:

- Your current default, Qwen3-0.6B Q4_K_M, is acceptable for lightweight text translation once Japanese text has already been extracted.
- It is small, so it won’t be the best for nuance, consistency, or long mixed-content sections.
- But it is not the core reason the screenshot still has Japanese.

What I’d recommend:

- For the current architecture, fix OCR first.
- After OCR is stable, upgrade the translator to a stronger local model.

Between Apple Intelligence and Gemma:

- Apple’s Foundation Models framework is an on-device text model for generation/understanding, not a replacement for OCR. You would still need OCR first. Apple also notes the o
  n-device model has more limited reasoning than larger server models. Sources: Foundation Models overview (https://developer.apple.com/documentation/FoundationModels), Prompti
  ng an on-device foundation model (https://developer.apple.com/documentation/foundationmodels/prompting-an-on-device-foundation-model)
- As of April 25, 2026, Google has officially released Gemma 4 on April 2, 2026, including multimodal variants that can handle images. Source: Gemma 4 announcement
  (https://blog.google/innovation-and-ai/technology/developers-tools/gemma-4/)
- More importantly for your use case, Google released TranslateGemma on January 15, 2026, specifically for translation across 55 languages. Source: TranslateGemma announcement
  (https://blog.google/innovation-and-ai/technology/developers-tools/translategemma/)

Best choice for this app:

- If you keep the current OCR -> translate text architecture:
  - Use a stronger local translation model, ideally TranslateGemma, not Apple Intelligence.
- If you want the model itself to read screenshots directly:
  - That is a different architecture. Then a multimodal local model like Gemma 4 could make sense, but you’d be replacing or bypassing OCR.

Practical recommendation:

1. Keep OCR and fix it first.
2. Replace Qwen3-0.6B Q4_K_M with a better local translator after that.
3. Prefer TranslateGemma over Apple Intelligence for this specific product goal.
4. Only move to Gemma 4 image-understanding if you want to redesign the pipeline around direct screenshot understanding.

If you want, I can next:

- instrument the OCR path so we can see exactly which Japanese lines are being dropped, or
- switch the app’s translation sidecar target from Qwen3 to a better local model plan.

---

Your project architecture is exceptionally robust for a macOS desktop application, but it is not yet distribution-ready. You have a solid core pipeline that follows high-quality platform
standards, but several critical "last-mile" items for a production product are still missing.

Production-Ready Strengths

- Native macOS Integration: Your use of ScreenCaptureKit for high-performance capture, Vision for OCR, and NSWorkspace/NSProcessInfo for context and thermal monitoring is "10/10" engineering
  for the Mac platform.
- Pipeline Robustness: The implementation of a watchdog for llama-server, health check retries, and explicit thermal/battery-aware throttling shows a high level of concern for long-running
  stability and user experience.
- Privacy & Context: Automatically clearing translation memory on app switches is a sophisticated touch that protects both LLM context quality and user privacy.
- Error Handling: You are using structured error reporting (anyhow) and a visible UI notification system (emit_runtime_notice) rather than letting the app fail silently.

Production Blockers (The "Missing 20%")

1.  Updater Security: tauri.conf.json contains an empty pubkey. You cannot securely distribute or update the app without a valid signing keypair.
2.  Frontend Security: There is no Content Security Policy (CSP) in index.html. For a production app, a restrictive CSP is mandatory to mitigate potential injection risks, even in a local-only
    app.
3.  Validation Debt: Your test-corpus consists of placeholder files. Without real regression fixtures, you cannot confidently guarantee that a change to your prompt or model doesn't break
    OCR/Translation quality for Japanese text.
4.  Distribution Infrastructure: The updater endpoint points to a placeholder URL. You need a real release pipeline for latest.json and .app.tar.gz files.
5.  Multi-Display Support: The current runtime targets display 0. On a production machine with multiple monitors, the app's behavior will be inconsistent or broken.
6.  Temporary File Handling: Storing frames in /tmp/contextura-frame-latest.png is functional but insecure on a shared system. Production apps should use a private App Data or Cache directory.

Summary of Readiness
┌───────────────┬───────────┬─────────────────────────────────────────────────────────────┐
│ Category │ Readiness │ Note │
├───────────────┼───────────┼─────────────────────────────────────────────────────────────┤
│ Architecture │ 9 / 10 │ Extremely well-decoupled and platform-native. │
│ Stability │ 8 / 10 │ Great watchdog/health check logic; needs more load testing. │
│ Security │ 4 / 10 │ Missing CSP and Updater signing keys. │
│ Testing │ 3 / 10 │ Code is testable, but actual regression data is missing. │
│ UX/Onboarding │ 7 / 10 │ Wizard is present, but model management is still manual. │
└───────────────┴───────────┴─────────────────────────────────────────────────────────────┘

Verdict: Your Architecture is production-ready, but your Configuration and Distribution setup is not. I recommend prioritizing the Updater Signing and CSP before any public release.
