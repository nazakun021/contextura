# ADR-0006: Fix Frame Deduplication with a Real Hash

**Status:** Accepted  
**Date:** 2026-07-04  
**Deciders:** Project owner  

## Context

The pipeline deduplicates frames to avoid re-running OCR+translation on identical screen content. The current implementation computes a "hash" by summing all pixel values in the 160×90 grayscale thumbnail:

```rust
let frame_hash = thumbnail.iter().map(|&pixel| u64::from(pixel)).sum::<u64>();
```

This is a **sum**, not a hash. Two frames with different pixel distributions but the same total brightness will produce the same value, leading to false-positive deduplication (skipping a frame that's actually different).

For a 160×90 = 14,400 pixel thumbnail with 8-bit values, the maximum sum is ~3.6M, which is a very small keyspace for a dedup check.

## Decision

Replace the sum with a proper hash function:

- **Preferred:** `xxHash` (via the `xxhash-rust` crate) — fast, no-allocation hashing designed for data integrity checks. Already commonly used in Rust ecosystems.
- **Alternative:** `FNV` (via `fnv` crate) — simpler, good for small data, but xxHash is faster for the ~14KB thumbnail.
- **Fallback:** `std::hash::DefaultHasher` — zero new dependencies but uses SipHash which is slower.

```rust
use xxhash_rust::xxh3::xxh3_64;
let frame_hash = xxh3_64(&thumbnail);
```

## Consequences

- **Positive:** Eliminates false-positive dedup collisions that could silently skip frames with different content.
- **Positive:** xxHash is faster than the current sum loop for this data size.
- **Cost:** One new dependency (`xxhash-rust`), which is lightweight and well-maintained.
