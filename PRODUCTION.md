# PRODUCTION.md — Road to 10/10

**Document Version:** 3.1.0
**Audit Date:** 2026-04-22
**Overall Health Score:** 5.5 / 10

---

## Overview

Tracks production readiness based on verified code state. Previous version (2.0.0) contained inaccurate ✅ statuses. Version 3.0.0 corrected those. Version 3.1.0 adds the **model architecture fix** (NLLB removed, Qwen3-0.6B introduced) as the new highest-priority blocker.

---

## Model Architecture Fix — New Top Priority

**Finding:** NLLB-200 is an encoder-decoder (seq2seq/BART) model. llama.cpp supports only decoder-only architectures. This incompatibility is architectural and cannot be resolved through configuration.

**Resolution:**

- Remove NLLB model file from `models/` directory
- Download `Qwen/Qwen3-0.6B-GGUF` Q4_K_M (~350MB) from Hugging Face
- Update `manifest.json` and `settings.json` to reference the new model
- Add `--jinja` to llama-server sidecar launch args
- Add `/no_think` to system prompt in `translation.rs`

No other code in the translation pipeline changes. HTTP API, payload format, and response parsing are identical.

---

## Current Blockers (Must fix to see any translation)

| Priority | Blocker                           | File                        | Status                           |
| -------- | --------------------------------- | --------------------------- | -------------------------------- |
| 1        | Wrong model architecture (NLLB)   | models/ + settings          | ❌ NLLB incompatible             |
| 2        | Shell capability missing          | `capabilities/default.json` | ❌ Missing `shell:allow-execute` |
| 3        | PNG snapshot never written        | `lib.rs`                    | ❌ Frame never saved             |
| 4        | Motion detection not instantiated | `lib.rs`                    | ❌ Never created in pipeline     |
| 5        | IPC events never emitted          | `lib.rs`                    | ❌ `app_handle.emit()` absent    |
| 6        | Styling never called              | `lib.rs`                    | ❌ `StylingEngine` dead code     |

---

## Full Status Table (Audit-Accurate)

| Consideration                    | Status                          | Priority            |
| -------------------------------- | ------------------------------- | ------------------- |
| Model architecture compatibility | ❌ NLLB wrong arch → Qwen3-0.6B | 🔴 Immediate        |
| Shell capabilities for sidecar   | ❌ Missing                      | 🔴 Immediate        |
| Pipeline end-to-end              | ❌ Not wired                    | 🔴 Immediate        |
| `--jinja` flag for Qwen3         | ❌ Not in sidecar args          | 🔴 Immediate        |
| `/no_think` in system prompt     | ❌ Not in prompt                | 🔴 Immediate        |
| `Cmd+Shift+T` toggle overlay     | ✅ Done                          | 🟢 Completed        |
| `Cmd+Shift+R` force OCR          | ✅ Done                          | 🟢 Completed        |
| Context `memory.clear()` wiring  | ✅ Done                          | 🟢 Completed        |
| Temp PNG cleanup                 | ✅ Done                          | 🟢 Completed        |
| Real `scale_factor` from display | ❌ Hardcoded 2.0                | 🟠 Phase P.Complete |
| Battery check in thermal         | ✅ Done                          | 🟢 Completed        |
| Sentry initialization            | ❌ Never called                 | 🟡 Step 7           |
| Watchdog health poll + restart   | ✅ Done                          | 🟢 Completed        |
| `excludedWindows` in capture     | ❌ Missing                      | 🟡 Phase 7          |
| Wizard screens 2–4               | ❌ Only screen 1                | 🟡 Phase 8          |
| Auto-updater pubkey              | ⚠️ Empty string                 | 🟡 Phase 8          |
| `--debug-cli` real output        | ❌ Stub                         | 🟡 Phase 7          |
| `--test-suite` real E2E          | ❌ Stub                         | 🟡 Phase 7          |
| Multi-display support            | ❌ Display 0 only               | 📋 v1.1             |
| Model switching (Quality Mode)   | ❌ No `switch_model()`          | 📋 v1.1             |
| RAM gate for Quality Mode        | ❌ Not implemented              | 📋 v1.1             |
| Apple Foundation Models          | ❌ Not started                  | 📋 v1.1             |

---

## 1. Model Architecture ❌ → Qwen3-0.6B

The NLLB model is physically incompatible with llama-server. It must be replaced.

**Positive side effect:** Qwen3-0.6B Q4_K_M is ~350MB — lighter than NLLB's 1.2GB. The Standard tier memory footprint drops from ~1.5GB to ~650MB. This gives more headroom on a loaded 16GB system.

**Qwen3-specific requirements:**

- `--jinja` flag in llama-server args (Jinja2 chat template required for Qwen3)
- `/no_think` in system prompt (disables thinking mode that would break response parsing)

---

## 2. Pipeline End-to-End ❌

All subsystems work individually. `lib.rs` never connects them. See TODO.md Phase P.Complete Steps 1–6 for the specific wiring work.

**What's needed in `lib.rs`:**

- `save_frame_as_png()` using the `image` crate
- `MotionDetector` + `DebounceStateMachine` instantiation
- `StylingEngine` instantiation + `par_iter()` call
- `app_handle.emit()` for all 4 event types
- `invalidation_rx` drain for context memory management

---

## 3. Shell Capabilities ❌

`capabilities/default.json` is missing `shell:allow-execute`. Without this, the `app.shell().sidecar("llama-server")` call silently fails in Tauri v2's permission model.

**Fix (one line to add):**

```json
"shell:allow-execute"
```

This must be fixed before pipeline testing. The sidecar will not spawn without it.

---

## 4. Remaining Items (Unchanged from v3.0.0)

### Wizard Screens 2–4 ❌

Screen 1 only. For local development: set `wizard_completed: true` in `settings.json`. Implement screens 2–4 before Phase 8.

### Hotkeys — Partial ⚠️

`Cmd+Shift+Q` and `Cmd+Shift+M` work. `T`, `R`, `G` are log stubs. `T` and `R` in Phase P.Complete Step 6. `G` (model switch) in v1.1.

### Watchdog ❌

No background `/health` polling. Add in Phase P.Complete Step 7.

### Sentry ❌

Dependency present. `sentry::init()` never called. Wire conditionally in Phase P.Complete Step 7.

### Auto-Updater ⚠️

Configured except for empty pubkey. Populate in Phase 8.

---

## Estimated Effort to Ship

| Work                                 | Estimate       |
| ------------------------------------ | -------------- |
| Model swap + Qwen3 config (today)    | 30 minutes     |
| Phase P.Complete (pipeline wiring)   | 1–3 days       |
| Phase 7 (hardening, real test suite) | 1 week         |
| Phase 8 (signing, wizard, packaging) | 3–5 days       |
| **Total to v1.0**                    | **~2–3 weeks** |

The model swap is the smallest item and unblocks everything else. Do it first today, then run the pre-flight checks, then start Phase P.Complete.
