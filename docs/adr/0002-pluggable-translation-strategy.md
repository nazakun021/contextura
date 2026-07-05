# ADR-0002: Pluggable Translation Strategy Pattern

**Status:** Accepted  
**Date:** 2026-07-04  
**Deciders:** Project owner  

## Context

The translation layer currently maintains two hardcoded code paths in `translation.rs`:
1. **TranslateGemma**: Sequential structured chat requests per string within each chunk.
2. **Qwen**: Numbered batched translation with rolling context memory and `/no_think`.

The active strategy is selected by inspecting the active model's ID string. Adding a third model family would require adding another branch to the conditional logic.

The ROADMAP Phase 3 mentions "Extended Model Manifests" for additional model families. The project owner wants to keep both strategies but make the system extensible.

## Decision

Extract a `TranslationStrategy` trait (or equivalent pattern) that encapsulates:
- How to format a translation request for a batch of source texts
- Whether to use `/no_think` or other model-specific flags  
- How to parse the model's response back into individual translations
- Required `llama-server` launch flags beyond the common set

The `ModelManifest` in `models.rs` would map each model entry to its strategy identifier. The `TranslationClient` would hold a `Box<dyn TranslationStrategy>` (or enum dispatch for zero-cost) and delegate formatting/parsing to it.

```rust
trait TranslationStrategy: Send + Sync {
    fn format_request(&self, texts: &[String], memory: &[MemoryEntry]) -> RequestPayload;
    fn parse_response(&self, response: &str) -> anyhow::Result<Vec<String>>;
    fn extra_launch_args(&self) -> Vec<String>;
}
```

## Consequences

- **Positive:** Adding a new model family becomes a single `impl TranslationStrategy` without touching the core pipeline.
- **Positive:** Each strategy can be unit-tested in isolation with mock HTTP responses.
- **Risk:** Over-abstraction if only 2-3 strategies ever exist. Mitigate by using an enum dispatch (`TranslateGemmaStrategy`, `QwenStrategy`) rather than full dynamic dispatch until the pattern proves its value.
- **Dependency:** This should be done *after* ADR-0001 (lib.rs split) to avoid conflicting refactors.
