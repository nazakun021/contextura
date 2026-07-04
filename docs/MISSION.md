# Mission

Contextura is a privacy-conscious, real-time Japanese-to-English screen translation overlay for macOS. It aims to bridge the language gap for English speakers reading Japanese text, playing games, or exploring visual media on Apple Silicon without relying on cloud translation APIs.

## Core Tenets

### 1. Offline-First & Privacy-Conscious
* All OCR processing and LLM translations must happen on-device using local resources (CPU, GPU, NPU).
* No data leaves the user's machine.
* **Telemetry**: Telemetry and crash reporting (e.g., Sentry) must be strictly opt-in. The app will explicitly ask for permission first, defaulting to disabled if the user does not opt in.

### 2. Zero-Friction User Experience
* Real-time translations are overlayed directly on top of the detected text blocks.
* Font size, background colors, and styling must dynamically adjust to ensure readability and high contrast (following WCAG guidelines).
* The overlay is toggled via global keyboard shortcuts (`Cmd+Shift+T`), minimizing interaction overhead.

### 3. Apple Silicon Native Performance
* Contextura is built specifically for macOS (macOS 13+) and optimized for Apple Silicon (M1/M2/M3/M4).
* It utilizes macOS-native OCR (`ScreenCaptureKit` and `Vision` framework) for high-performance frame capture and analysis.

### 4. Direct Resource Management
* GGUF models (specifically `TranslateGemma`) are downloaded directly from authenticated remote sources (e.g., Hugging Face) via the app.
* Sidecars and capture streams are managed cleanly to conserve battery and control RAM usage.
