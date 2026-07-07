# ADR-0012: Selective Translation and Overlay Accuracy Testing

**Status:** Accepted  
**Date:** 2026-07-07  
**Decision Makers:** User + Agent  
**Context:** Grilling session on mixed-language overlay behavior and test corpus gaps

## Context

Users report that English-only text sometimes receives a translation overlay on mixed-language screens (e.g., a Japanese game with English HUD elements). The pipeline's CJK filter in `ocr.rs` (`contains_cjk`) should already drop purely-English boxes, but something is leaking through. The exact failure mode is unconfirmed — it could be Vision OCR returning CJK-tagged results for English text, a coordinate/sizing issue, or a filter gap.

Separately, the test corpus has `ocr_boxes` coordinate assertions in its schema but none of the 3 existing cases use them (`"ocr_boxes": []`). There is no way to verify that English text is *not* being overlayed, and no visual debug output for inspecting box placement.

## Decision

### 1. Build four mixed-language test cases

New fixture cases that combine Japanese and English text in realistic scenarios:

| Case | Scenario | Purpose |
|------|----------|---------|
| `case4-game-mixed` | Japanese dialog + English HUD (HP bars, level numbers, menu labels) | Game UI with spatial separation between languages |
| `case5-webpage-mixed` | Japanese article body + English navigation/headers | Bilingual web layout |
| `case6-subtitle-mixed` | Japanese subtitles over English media player UI chrome | Overlay-over-chrome scenario |
| `case7-code-mixed` | Japanese comments alongside English code | IDE/terminal scenario |

### 2. Extend fixture schema with negative assertions

Add two new fields to `CorpusExpectation`:

- **`ocr_must_not_contain`**: `Vec<String>` — Each fragment must NOT appear as a substring in the concatenated OCR text output. Verifies that the CJK filter is rejecting English-only detections.
- **`ocr_boxes_must_not_exist`**: `Vec<ExpectedOcrBox>` — No detected OCR box should match these entries (same ±5px coordinate tolerance). Verifies that overlay boxes are not placed over English text regions.

### 3. Always output annotated PNGs

When running `--test-suite`, generate `<case>.annotated.png` for every test case:

- **Green rectangles**: Detected OCR boxes that matched an expected box
- **Red rectangles**: Detected OCR boxes that matched a `must_not_exist` entry, or expected boxes that weren't found
- **Blue rectangles**: Detected OCR boxes with no corresponding assertion (informational)
- Output goes alongside the source PNGs in the test-corpus directory

### 4. Investigate and fix the English overlay leak

Use the new mixed-language fixtures to reproduce the leak, then fix. The investigation should cover:

- Vision helper returning CJK-tagged results for English text
- The `contains_cjk` character range (does it cover all edge cases?)
- Coordinate mapping errors causing box misplacement

### 5. Mixed CJK+English handling

Boxes containing a mix of CJK and English characters (e.g., `"生成AI (ChatGPT)"`) should continue to be translated. The `contains_cjk` filter's current behavior of keeping any box with at least one CJK character is intentional. Only purely-English boxes should be filtered out.

## Consequences

- The test corpus grows from 3 to 7 cases, with all new cases using `ocr_boxes` and `ocr_boxes_must_not_exist` assertions.
- The `--test-suite` runner gains annotated PNG output, making overlay placement visually verifiable.
- The `CorpusExpectation` struct gains two new optional fields; existing fixtures are unaffected (they default to empty).
- The `CaseResult` struct gains a new `negative_text_ok` and `negative_box_ok` field.
- `cli.rs` and `ocr.rs` are the primary files affected.
- `gen_fixtures.swift` needs extending to generate the new mixed-language PNGs.

## Alternatives Considered

- **CJK-ratio threshold** (e.g., only keep boxes where >50% of characters are CJK): Rejected in favor of reproducing the actual bug first. The `contains_cjk` filter is working as designed; the leak likely has a different root cause.
- **HTML debug report**: Rejected in favor of annotated PNGs, which are self-contained and don't require a browser.
- **Opt-in annotated output via `--visualize` flag**: Rejected; always generating annotated PNGs adds negligible cost and ensures visual inspection is always available.
