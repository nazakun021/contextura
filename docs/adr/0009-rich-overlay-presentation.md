# ADR-0009: Rich Overlay with Animations and Collision Avoidance

**Status:** Accepted  
**Date:** 2026-07-04  
**Deciders:** Project owner  

## Context

The current overlay (`overlay.js`, 2.7KB) is a passive renderer — it receives `translation-update` events and paints absolutely positioned `<div>` elements. There are no:
- Transitions when boxes appear, disappear, or move
- Fade effects when translations update
- Loading states while translation is in progress
- Collision detection to prevent overlapping boxes
- Animation for position changes between frames

The MISSION.md tenet "Zero-Friction User Experience" and "dynamically adjust to ensure readability" implies more visual sophistication than the current implementation provides.

The project owner wants the overlay to own more presentation logic, specifically: animation timing, box fade transitions, and layout collision avoidance.

## Decision

Evolve the overlay from a passive renderer to a smart presentation layer:

### Phase 1: Smooth Transitions
- Add CSS transitions for box opacity (fade in/out) and position (slide on update).
- Animate box removal with a brief fade-out instead of instant DOM removal.
- Show a subtle loading indicator (e.g., pulsing border or skeleton text) during the `translation-started` → `translation-update` gap.

### Phase 2: Collision Avoidance
- Detect overlapping styled boxes and nudge them apart vertically or horizontally.
- Prefer shifting boxes downward rather than obscuring the original Japanese text.
- Keep collision logic in JS — it's a presentation concern, not a backend concern.

### Phase 3: Progressive Rendering (depends on ADR-0008)
- Display styled boxes with placeholder text immediately after OCR.
- Fill in translated text as it arrives from the LLM, with a subtle text-swap animation.

The frontend remains vanilla HTML/CSS/JS — no framework needed for these features.

## Consequences

- **Positive:** Dramatically improves the perceived quality and professionalism of the overlay.
- **Positive:** Collision avoidance prevents unreadable stacked boxes in dense Japanese text regions.
- **Risk:** Animation timing must be calibrated carefully — too slow feels laggy, too fast feels jittery. Target 150-200ms for transitions.
- **Risk:** Collision avoidance adds computational cost per frame update. Profile on large OCR results (20+ boxes).
