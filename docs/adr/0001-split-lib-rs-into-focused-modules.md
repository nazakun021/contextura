# ADR-0001: Split lib.rs into Focused Modules

**Status:** Accepted  
**Date:** 2026-07-04  
**Deciders:** Project owner  

## Context

`lib.rs` has grown to 1,320 lines (53KB). It currently contains:
- Tauri app setup and plugin registration (~100 lines)
- Binary resolution helpers (3 functions, ~60 lines)
- Frame processing utilities: `swap_bgra_to_rgba`, `save_frame_as_png`, `cleanup_stale_temp_frames` (~60 lines)
- `process_capture_frame` — the core OCR→translate→style→emit pipeline (~150 lines)
- `run()` — the main entry point, including the entire outer pipeline loop, sidecar lifecycle, watchdog spawn, and inner capture loop (~560 lines)
- Tauri commands: `complete_wizard`, `wizard_status`, `open_models_folder_command`, etc. (~80 lines)
- CLI debug mode: `run_debug_cli_once`, `run_test_suite`, `run_cli` (~250 lines)

The AGENTS.md rule "keep application orchestration in lib.rs" was meant to prevent scattering coordination logic, but the file now also contains business logic (frame processing, sidecar lifecycle) and utility code (binary resolution, pixel conversion) that are not orchestration.

## Decision

Split `lib.rs` into focused modules along these natural seams:

| New Module | Responsibility |
| :--- | :--- |
| `lib.rs` | Tauri setup, plugin registration, `run()` entry point, command handler registration. Thin orchestration shell. |
| `pipeline.rs` | The pipeline state machine: outer runtime loop, inner capture loop, pipeline commands, frame dedup. |
| `frame.rs` | Frame utilities: BGRA↔RGBA conversion, PNG snapshot writing, temp file cleanup. |
| `sidecar.rs` or extend `translation.rs` | Sidecar lifecycle orchestration: startup, ready-wait, watchdog loop. |
| `resolve.rs` | Binary resolution helpers for `vision-helper` and `llama-server`. |

The CLI code in `run_cli`, `run_debug_cli_once`, and `run_test_suite` already has its own module (`cli.rs`) but the implementations live in `lib.rs`. Move them into `cli.rs`.

## Consequences

- **Positive:** Each module has a clear single responsibility. The inner capture loop can be tested independently.
- **Positive:** Pipeline logic becomes unit-testable without standing up a full Tauri app.
- **Risk:** Extracting the pipeline loop means passing many dependencies (app_handle, client, ocr_engine, etc.). Consider a `PipelineContext` struct to bundle them.
- **Migration:** This is a pure refactor with no behavioral changes. Can be done incrementally, one extraction at a time.
