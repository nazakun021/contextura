# ADR-0007: Eliminate Redundant BGRA→RGBA Conversions

**Status:** Accepted  
**Date:** 2026-07-04  
**Deciders:** Project owner  

## Context

ScreenCaptureKit produces frames in BGRA format. The pipeline currently converts BGRA→RGBA in multiple places:

1. **Normal capture path** (line ~976): Converts BGRA→RGBA to compute motion detection thumbnails, even though motion detection only uses grayscale (it doesn't need RGBA at all).
2. **`process_capture_frame`** (line ~241): Converts BGRA→RGBA again for PNG encoding and styling.
3. **Force-scan path** (line ~873): Converts the cached frame's BGRA→RGBA a third time for motion hash computation.

Each conversion copies the entire frame buffer (4 bytes × width × height). For a 2560×1600 Retina display, that's ~16MB per conversion.

## Decision

Restructure the pixel format handling:

**Option A (preferred): Convert once at capture time.**
- In `capture.rs`, perform the BGRA→RGBA swap immediately when copying pixels out of the ScreenCaptureKit sample buffer. The `CaptureFrame.data` field becomes RGBA.
- All downstream consumers (motion, OCR, styling) receive RGBA directly.
- Motion detection's grayscale conversion works identically on RGBA (it's just `0.299*R + 0.587*G + 0.114*B` regardless of channel order).

**Option B: Work in BGRA throughout.**
- Teach the PNG encoder and `vision-helper` to accept BGRA.
- Teach the styling sampler to read BGRA.
- This avoids any conversion but requires auditing every color-dependent consumer.

Option A is simpler because all existing code already assumes RGBA.

## Consequences

- **Positive:** Eliminates ~32-48MB of redundant memcpy per OCR cycle on a Retina display.
- **Positive:** Simplifies the pipeline — `swap_bgra_to_rgba` is called once in capture, not scattered across the pipeline.
- **Risk:** The BGRA→RGBA swap in `capture.rs` happens on the capture thread, which could add latency to frame delivery. Profile to verify this is acceptable.
- **Dependency:** Coordinates with ADR-0001 (lib.rs split) since `swap_bgra_to_rgba` moves to the capture module.
