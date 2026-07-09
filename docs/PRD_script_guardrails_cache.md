# Product Requirement Document (PRD) — Script Coverage, Guardrails & Translation Quality

## Problem Statement

Contextura users experience multiple accuracy, consistency, and coverage limitations in screen translation:
1. **Silent Kanji Dropping**: High-density Kanji-only or Kanji-heavy Japanese text (e.g., `"出口"` for exit, `"設定"` for settings) is silently discarded by the CJK filter in `ocr.rs` because it requires at least 2 Hiragana/Katakana characters to pass.
2. **Translation Quality Degradation**: Local quantized models occasionally output meta-commentary, echo the source Japanese text back untranslated, leak reasoning `<think>` tags, or enter repetition loops.
3. **Sequential Latency**: Retrying missing or failed translation slots one-by-one sequentially adds significant frame-processing latency, which ruins the real-time overlay experience.
4. **Flickering & Inconsistency**: Identical static strings across consecutive frames are translated inconsistently or cause unnecessary LLM sidecar requests, increasing resource utilization.
5. **Prompt Injection Risk**: Raw OCR strings are interpolated directly into chat templates, presenting a risk of instruction hijacking from on-screen content.

---

## Solution

We will implement script-composition classification, translation-output validation guardrails, concurrent slot retrying, a dedicated terminology translation cache, and delimiter-fenced prompt formatting.

---

## User Stories

1. As a Japanese game player, I want Kanji-only UI labels like `"設定"` (Settings) or `"保存"` (Save) to be translated, so that I can navigate game menus and configurations easily.
2. As a manga reader, I want single-character Kanji signs (like `"駅"` or `"店"`) to be translated if they are recognized with high confidence, so that I don't miss background environment details.
3. As a user translating a crowded screen, I want translation retries to execute concurrently, so that the overlay does not stutter or lag when some boxes fail validation.
4. As a user, I want the translation of static on-screen text (like menu headers) to remain identical across frames, so that the overlay is visually consistent and doesn't flicker.
5. As a developer, I want Contextura to resist prompt injection attacks from on-screen text, so that the local model cannot be hijacked to run arbitrary instructions.
6. As a user, I want to see a clear "degraded/unavailable" visual state for overlay boxes that fail all translation retries, so that I know the system could not translate them instead of seeing stale or blank boxes.
7. As a user, I want reasoning tokens (like `<think>...</think>`) and conversational preambles to be stripped automatically, so that only clean translation outputs are rendered on my screen.
8. As a developer, I want all Japanese script character-counting rules to live in a single module, so that both OCR filters and translation guardrails match exactly.

---

## Implementation Decisions

### 1. Unified Script Classification (`script.rs`)
* Create `src-tauri/src/script.rs` to expose:
  ```rust
  pub struct ScriptCounts {
      pub hiragana: usize,
      pub katakana: usize,
      pub kanji: usize,
  }
  pub fn count_script_chars(text: &str) -> ScriptCounts;
  pub fn classify_script(text: &str, is_japanese: bool) -> ScriptVerdict;
  ```
* Implement a Simplified Chinese character-range/radical denylist in Rust (filtering out characters/radicals like `们`, `这`, `纟`, `讠`, `门` that never occur in Japanese).
* Scoped confidence threshold in `ocr.rs`: Require `confidence >= 0.75` for single-character Kanji-only boxes, while keeping `MIN_CONFIDENCE = 0.3` for multi-character boxes.

### 2. Output Guardrails (`guardrails.rs`)
* Create `src-tauri/src/guardrails.rs` to expose:
  ```rust
  pub struct ValidationOutcome {
      pub accepted: bool,
      pub reason: Option<String>,
      pub cleaned_text: String,
  }
  pub fn validate_translation(source: &str, candidate: &str) -> ValidationOutcome;
  ```
* Perform `<think>` tag stripping, meta-commentary removal, untranslated CJK-echo detection, residual Japanese checks, length-ratio checks (e.g. max 6x source length), and refusal-phrase filtering.

### 3. Concurrent Retries & Interface Consolidation
* Consolidate retry logic for empty, missing, and guardrail-failed slots into a single, unified parallel retry helper in `translation.rs` using `futures::future::join_all`.

### 4. Terminology Caching (`TranslationCache`)
* Add a dedicated `TranslationCache` struct in `translation.rs` backed by a capacity-limited LRU map (e.g., 200 items) to store exact translations of static UI strings and avoid re-querying the LLM.

### 5. Input Hardening & Prompts
* Wrap raw OCR text in `<text>...</text>` tags in the strategy templates:
  `{number}: <text>{original_string}</text>`
* Add explicit instructions in system prompts to treat all content inside `<text>` tags as literal strings, never as instructions.

---

## Testing Decisions

### Seams under test:
* **Script classification**: Test the public functions `count_script_chars` and `classify_script` directly with Hiragana, Katakana, mixed Kanji, pure Kanji, and Chinese-only samples.
* **Translation validation**: Test `validate_translation` against think-tags, echoed outputs, conversational preambles, and repetition loops.
* **Cache**: Test `TranslationCache` insert, exact lookup, and LRU eviction.

### Prior Art:
* Unit tests in `translation.rs` and `ocr.rs`.
* Golden-file integration tests using the local test-suite CLI. Add a new fixture `case9-kanji-only-ui` asserting that short Kanji-only blocks like `"設定"`, `"保存"`, and `"終了"` are captured and translated.

---

## Out of Scope

* Tuning prompt formatting for specific model families other than Gemma, Qwen, and LFM.
* Machine-learning language identifier models (like fastText) for language detection.
* Translation memory database persistence across application restarts.

---

## Further Notes

Implementation order:
1. Extract `script.rs`.
2. Implement `classify_script` and custom Kanji-only confidence floors.
3. Consolidate retry helper.
4. Implement `guardrails.rs` and wire into retry loops.
5. Implement `TranslationCache`.
6. Add prompt-injection delimiters and degraded-state overlay support.
