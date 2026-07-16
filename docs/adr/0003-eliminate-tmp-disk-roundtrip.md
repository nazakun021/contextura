# ADR-0003: Eliminate the /tmp Disk Round-Trip for OCR

**Status:** Accepted (partially implemented)  
**Date:** 2026-07-04  
**Deciders:** Project owner

## Context

The current OCR pipeline:

1. Converts BGRA capture frame to RGBA in memory.
2. Encodes RGBA to PNG and writes it to `/tmp/contextura-frame-{id}.png`.
3. Invokes `vision-helper` as a subprocess, passing the file path as an argument.
4. `vision-helper` reads the PNG from disk, decodes it, and passes it to Apple Vision.
5. Returns OCR results as JSON on stdout.

This disk round-trip:

- Adds latency (~5-20ms for encode + write + read + decode on SSD).
- Creates files in an insecure shared directory (`/tmp`). TECH-STACK.md already flags this.
- Produces a redundant PNG decode inside the Swift helper (the Rust side already has the raw pixels).

The project owner confirmed this bothers them and they'd prefer a direct-memory path.

## Decision

**Phase 1 (immediate):** Move temp files from `/tmp` to the app's private cache directory (`~/Library/Caches/contextura/`). This addresses the security concern without changing the architecture.

**Phase 2 (implemented):** Runtime now encodes frames to PNG bytes in memory and streams them to `vision-helper --stdin`, removing file-path handoff in the OCR hot path.

**Future options:**

- Option A: Stream raw RGBA bytes + dimensions header via stdin to avoid PNG encode/decode overhead.
- Option B: Use a memory-mapped file in the app's private directory.
- Option C: Embed the Vision OCR call directly in Rust via `objc2` bindings, eliminating the subprocess entirely.

Keep any debug frame-file output as an opt-in debug feature, not as a pipeline dependency.

## Consequences

- **Positive (Phase 1):** Eliminates the shared `/tmp` security concern.
- **Positive (Phase 2):** Reduced per-frame latency by removing file-path roundtrip from the OCR hot path.
- **Risk (Phase 2):** The Apple Vision framework expects `CGImage`/`CIImage` inputs. Creating these from raw pixel data in Swift requires careful memory management. Investigation needed.
- **Risk (Phase 2, Option C):** Calling Vision from Rust via `objc2` is complex and would eliminate the clean subprocess boundary that makes `vision-helper` independently testable.
