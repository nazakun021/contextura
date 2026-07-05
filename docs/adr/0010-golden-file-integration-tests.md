# ADR-0010: Golden-File Integration Tests for OCR+Translation Pipeline

**Status:** Accepted  
**Date:** 2026-07-04  
**Deciders:** Project owner  

## Context

The current test strategy is almost entirely manual:
- `cargo test` runs only basic compile-time checks.
- `cargo clippy` catches lint issues.
- The `test-corpus/*.png` files are empty placeholders.
- The `--test-suite` CLI path is wired but produces no useful results.
- Full verification requires launching the GUI, having a model installed, and visually checking overlay output.

The project owner wants **integration tests with real captured frames** (golden files) as the priority, specifically to validate that the OCR+translation pipeline produces correct output for known inputs.

## Decision

Build a golden-file integration test suite:

### Test Structure
```
test-corpus/
├── manga-speech-bubble/
│   ├── input.png           # Real screenshot with Japanese text
│   ├── expected-ocr.json   # Expected OCR boxes (text + coordinates)
│   └── expected-translations.txt  # Expected English translations (one per line)
├── game-dialog/
│   ├── input.png
│   ├── expected-ocr.json
│   └── expected-translations.txt
└── website-article/
    ├── input.png
    ├── expected-ocr.json
    └── expected-translations.txt
```

### Test Levels

1. **OCR-only tests** (no sidecar needed): Run `vision-helper` on `input.png` and compare against `expected-ocr.json`. Assert text matches and bounding boxes are within tolerance.
2. **Full pipeline tests** (sidecar needed): Run the `--debug-cli --input` path and compare against both OCR and translation expectations. These require a model to be installed and run slower.
3. **Styling tests** (unit): Feed synthetic RGBA buffers and bounding boxes to `StylingEngine` and assert WCAG contrast ratios.

### Golden File Management
- Golden files are checked into git as real PNG screenshots (not empty placeholders).
- OCR expectations are semi-fuzzy: text content must match, but coordinates use a tolerance (±5px).
- Translation expectations use semantic comparison (exact match is too brittle for LLM output). Consider BLEU score or key-term matching.

### CI Integration
- OCR-only tests run in CI without a model (just `vision-helper` binary).
- Full pipeline tests run locally or in a dedicated CI job with a model pre-installed.

## Consequences

- **Positive:** Provides real regression detection for the most critical path in the app.
- **Positive:** The `--test-suite` CLI path already exists and just needs real fixtures.
- **Risk:** Translation golden files are inherently brittle — LLM output varies. Mitigate with fuzzy matching.
- **Risk:** Real PNG screenshots may contain copyrighted content. Use self-authored test images or screenshots of freely licensed content.
- **Cost:** Initial effort to capture, annotate, and verify 5-10 golden test cases.
