# ADR-0008: Pipeline OCR+Styling Before Translation Completes

**Status:** Accepted  
**Date:** 2026-07-04  
**Deciders:** Project owner  

## Context

`process_capture_frame` currently executes all stages serially:

```
save PNG → OCR → translate_batch → style all boxes → emit
```

But styling (sampling background colors and computing WCAG-compliant foreground colors) depends only on:
- The RGBA buffer (available before OCR starts)
- The bounding boxes (available when OCR completes)

Translation depends only on:
- The text strings (available when OCR completes)

Styling and translation are **independent of each other**. Running them serially means the user waits for `translate_batch` to finish before any styled boxes appear.

## Decision

After OCR completes, run styling and translation concurrently:

```
save PNG → OCR → ┬─ translate_batch ──┐
                  └─ style all boxes ──┘→ merge → emit
```

Implementation approach:
1. After OCR returns text boxes, immediately spawn styling as a `tokio::spawn` or rayon parallel task.
2. Simultaneously send the text batch to the translation client.
3. Join both results and merge into `TranslationBox` payloads.

Optionally (future): emit a preliminary "loading" payload with styled boxes but placeholder text, then update with translations as they arrive.

## Consequences

- **Positive:** Reduces perceived latency — styling is fast (microseconds per box) but translation can take seconds for large batches.
- **Positive:** Prepares the architecture for progressive rendering (showing "translating..." placeholders).
- **Risk:** Minimal — styling has no side effects and translation has no dependency on styling results.
- **Dependency:** Benefits from ADR-0001 (lib.rs split) since the pipeline function becomes cleaner when extracted.
