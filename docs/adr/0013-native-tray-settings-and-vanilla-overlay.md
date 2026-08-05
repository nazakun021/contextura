# ADR-0013: Native Tray Settings and Vanilla Overlay

**Status:** Accepted  
**Date:** 2026-08-05  
**Deciders:** Project owner

## Context

Contextura's transparent overlay is a small event-driven renderer. It receives
translation events, positions boxes, and normally ignores cursor events so the
user can continue interacting with the underlying application. The settings
needed for the first release are choosing an installed model and selecting how
translated text is placed relative to the detected Japanese text.

Adding an interactive in-overlay menu would require cursor-mode transitions
that risk intercepting clicks intended for the underlying application. A web
framework would not remove that macOS interaction constraint, and the existing
frontend has no routing, component tree, or client-side state complexity that
would justify its cost.

## Decision

- Keep the translation overlay as vanilla HTML, CSS, and JavaScript.
- Use the native Tauri tray as the configuration surface.
- Offer installed-model selection and three placement modes: cover, above, and
  below.
- Persist placement in `settings.json`; model selection continues to persist in
  the model manifest and settings.
- Keep `cover` placement at the exact OCR coordinates, including when Styled
  Boxes overlap. Apply collision avoidance only to `above` and `below`
  placement modes.

## Consequences

- The overlay remains click-through and lightweight.
- Selecting a model restarts the local runtime; placement updates take effect
  on the next rendered translation event.
- Cover mode prioritizes accurate source-text replacement over separating
  overlapping translations.
- A separate settings window remains a future option. Keep it vanilla unless
  it grows into validation-heavy workflows, model download management,
  profiles, search, or rich live previews that materially benefit from a
  component framework.
