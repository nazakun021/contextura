# ADR-0004: Event-Driven Capture Loop with tokio::select!

**Status:** Accepted  
**Date:** 2026-07-04  
**Deciders:** Project owner  

## Context

The inner capture loop in `lib.rs` currently uses a poll-based approach:

```rust
'capture: loop {
    sleep(Duration::from_millis(10)).await;
    while let Ok(command) = pipeline_rx.try_recv() { ... }
    let frame_res = frame_rx.try_recv();
    ...
}
```

This pattern:
- Wastes ~100 wakeups/second even when the screen is static and no commands are pending.
- Makes timing-sensitive behavior (debounce) harder to reason about because tick frequency is coupled to the sleep duration.
- Introduces subtle ordering bugs: pipeline commands are drained before frame processing, but a `ForceScan` arriving mid-frame-processing won't be seen until the next tick.

The project owner confirmed they'd prefer a proper event-driven approach.

## Decision

Replace the poll loop with `tokio::select!` multiplexing over:
1. The frame receive channel (converted to a tokio-compatible async channel or wrapped).
2. The pipeline command channel (similarly converted).
3. A debounce timer that fires when the settling period expires.

```rust
loop {
    tokio::select! {
        frame = frame_rx.recv() => { /* process frame, update motion/debounce */ }
        command = pipeline_rx.recv() => { /* handle ForceScan/Reload/Shutdown */ }
        _ = debounce_timer.tick() => { /* fire OCR on settled screen */ }
    }
}
```

This requires converting the crossbeam channels to tokio channels (or using `tokio::sync::mpsc`) since `tokio::select!` needs futures, not blocking `try_recv()`.

## Consequences

- **Positive:** Zero CPU cost when idle. The loop only wakes on actual events.
- **Positive:** Debounce timing becomes precise — a tokio `Sleep` future fires at exactly the settling deadline instead of being checked every 10ms.
- **Positive:** Commands are processed immediately, not on the next poll tick.
- **Risk:** Requires changing `crossbeam_channel` to `tokio::sync::mpsc` for the pipeline command channel. The capture frame channel in `capture.rs` may also need to become async.
- **Dependency:** Should be done as part of ADR-0001 (lib.rs split) since the capture loop is the primary extraction target.
