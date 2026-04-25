# PRODUCTION.md — Road to 10/10

**Document Version:** 4.1.0  
**Audit Date:** 2026-04-25  
**Overall Health Score:** 7.0 / 10

## Overview

The core single-display product path is wired in code, but it is not yet production-ready. Capture, motion gating, OCR, translation, styling, model switching, watchdog recovery, and overlay rendering are present. The standalone OCR helper runtime defect has been fixed in this workspace, but the bundled OCR corpus is still unusable and the end-to-end translation smoke pass still needs to be re-run with a healthy local model.

## Current Readiness

| Area                            | Status | Notes                                                          |
| ------------------------------- | ------ | -------------------------------------------------------------- |
| Decoder-only model architecture | ✅     | Qwen3 path remains correct for `llama-server`                  |
| Shell capabilities for sidecar  | ✅     | `shell:allow-execute` and `shell:allow-spawn` present          |
| End-to-end pipeline wiring      | ✅     | `lib.rs` drives capture → OCR → translation → overlay          |
| OCR helper reliability          | ✅     | Standalone `vision-helper` now succeeds on a saved live frame  |
| Watchdog + restart              | ✅     | Health failures emit a visible notice and restart the sidecar  |
| Overlay exclusion from capture  | ✅     | Capture excludes Contextura app windows                        |
| Model switching                 | ✅     | `Cmd+Shift+G` cycles to the next installed GGUF                |
| Wizard screens 1–4              | ✅     | Setup now covers permissions, model, controls, and ready state |
| Real CLI/test corpus flow       | ⚠️     | Code path is live, but the bundled corpus is currently invalid |
| Sleep/wake capture recovery     | ✅     | Stalled capture stream triggers restart logic                  |
| Manual live smoke verification  | [-]    | Still required with a healthy local model                      |
| Updater signing pubkey          | [ ]    | `tauri.conf.json` still has an empty pubkey                    |
| Curated quality-tier policy     | [ ]    | Switching exists, but no RAM gate or curated tier contract     |
| Multi-display support           | [ ]    | Runtime still targets display 0                                |

## What Changed in This Audit

- Removed the old stale blocker list that still claimed the pipeline was unwired.
- Promoted the implemented work now present in code: capture exclusion, visible runtime notices, live model switching, wizard expansion, CLI wiring, and capture restart handling.
- Kept only blockers that are still real after this code pass.

## Remaining Blockers

### 1. Manual smoke verification

Rust tests and clippy pass, but the app still needs live validation against real Japanese content with:

- Screen Recording permission granted
- A valid GGUF model installed
- Successful overlay alignment and translation over actual on-screen text

### 2. Test corpus quality

The checked-in `test-corpus/*.png` files are empty placeholders right now. That means the CLI and corpus harness exist, but they are not yet meaningful regression checks.

### 3. Updater signing

The updater plugin is initialized, but production distribution is still blocked on a real signing keypair and a non-empty public key in `tauri.conf.json`.

### 4. Quality-tier contract

The app can now cycle installed models, but product-grade tiering still needs:

- a curated Standard/Quality manifest policy
- RAM or device gating before switching to heavier models
- user-facing tier semantics in docs and onboarding

### 5. Multi-display

Capture and overlay logic still assume one display. Shipping broader desktop use confidently will require explicit display selection/routing.

## Verification Snapshot

Most recent verification in this workspace:

- `cargo test --manifest-path src-tauri/Cargo.toml`
- `cargo check --manifest-path src-tauri/Cargo.toml`

Not yet verified in this workspace:

- live runtime translation pass
- model switching during a running GUI session
- capture restart behavior after real sleep/wake

## Ship Criteria

To move from this state to a production-ready 1.0:

1. Replace the placeholder `test-corpus/` PNGs with real Japanese screenshots plus expected outputs.
2. Run the full manual smoke pass and record outcomes in `TODO.md` / `SPEC.md`.
3. Provision updater signing keys and wire the real public key into `tauri.conf.json`.
4. Decide whether model switching is generic or tiered, then encode that contract in manifest/settings/UI.
5. Either implement multi-display support or explicitly freeze single-display scope for 1.0 packaging.
