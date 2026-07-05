# ADR-0005: Fail-Loud Error Recovery for Translation Sidecar

**Status:** Accepted  
**Date:** 2026-07-04  
**Deciders:** Project owner  

## Context

The translation sidecar (`llama-server`) can fail in several ways:
1. Model file missing or corrupt → sidecar won't start.
2. Sidecar crashes mid-session → health checks fail.
3. Sidecar becomes unresponsive → health checks timeout.

The current codebase has three overlapping recovery mechanisms:
- **Watchdog task**: Checks health every 5s, restarts after 3 consecutive failures. Shows a warning notice.
- **Ready-check retry**: On initial startup, retries 5 times, then breaks out of the capture loop. Shows an error notice.
- **Runtime reload**: `PipelineCommand::ReloadRuntime` breaks the capture loop and re-enters the outer loop, which re-initializes the sidecar.

The project owner wants **fail-loud** behavior: if the sidecar can't run, the user should know immediately and unambiguously.

## Decision

Adopt a **fail-loud, user-visible** error philosophy:

1. **Startup failure**: If the sidecar fails to start or become ready within the configured timeout, display a persistent, prominent error in the overlay (not a transient toast). The error should include actionable guidance: "Model file may be missing. Open the models folder to verify."
2. **Runtime failure**: If the watchdog detects 3 consecutive failures, display a persistent error and stop attempting translation until the user explicitly retriggers (e.g., via `Cmd+Shift+R` or tray action).
3. **No silent degradation**: Do not silently fall back to "OCR-only" mode. The user must understand that translation is broken.
4. **Recovery trigger**: The user can manually retry via `Cmd+Shift+R` or the tray "Translate Now" action. The watchdog does not auto-retry indefinitely.

## Consequences

- **Positive:** Users always know when translation is broken instead of wondering why overlay boxes aren't appearing.
- **Positive:** Eliminates the confusing gap between "the app is running" and "translation is silently failing."
- **Risk:** A persistent error overlay could be annoying if the sidecar fails intermittently. Mitigate with a clear "Retry" affordance.
- **Change:** The watchdog's current "restart silently and hope" approach needs to be replaced with explicit user notification and manual retry.
