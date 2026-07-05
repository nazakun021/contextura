# ADR-0011: Remove Settings Hot-Reload Timer

**Status:** Accepted  
**Date:** 2026-07-04  
**Deciders:** Project owner  

## Context

The pipeline's outer loop refreshes `RuntimeState` (settings + active model) every 60 seconds via `SETTINGS_REFRESH_INTERVAL`. This was intended to pick up settings changes without restarting the app.

However:
- The most impactful setting change (active model) already triggers a `PipelineCommand::ReloadRuntime` which breaks the capture loop and restarts with new settings.
- The remaining settings (debounce_ms, motion_threshold, etc.) are tuning parameters that change infrequently.
- The 60-second timer adds complexity to the outer loop and creates a class of bugs where "settings changed but haven't been picked up yet."

The project owner confirmed that restarting the app for settings changes is acceptable for now.

## Decision

Remove `SETTINGS_REFRESH_INTERVAL` and the periodic settings reload. Settings are loaded:
1. Once at startup.
2. On explicit `PipelineCommand::ReloadRuntime` (triggered by model switch or user action).

If live settings reload is needed later, implement it as a file watcher on the settings file (using `notify` crate) that sends a `ReloadRuntime` command, rather than a timer-based poll.

## Consequences

- **Positive:** Simplifies the outer loop — removes the `should_refresh_runtime` check and the `loaded_at` timestamp.
- **Positive:** Settings changes take effect immediately (on reload) instead of "within 60 seconds."
- **Risk:** If a user manually edits the settings file, they'll need to restart the app. This is acceptable for a power-user-only action.
