# Improvement Audit

**Reviewed:** 2026-08-05

This audit records improvements found by comparing the checked-in configuration, runtime code, CI workflow, and local verification results. It is a prioritized backlog, not a claim that the items are complete.

## Implementation Update: 2026-08-05

The following safeguards are now implemented:

- Runtime settings tests explicitly cover AC and battery debounce behavior; the full suite passed with 136 unit tests and 2 CLI integration tests.
- `--debug-cli --validate-corpus-fixtures <DIR>` validates corpus fixture pairs and expectation JSON without a local model.
- `CONTEXTURA_DATA_DIR` provides an absolute, isolated data directory for headless commands and provisioned CI runners.
- The Wizard script is same-origin, and the Tauri CSP now disallows inline scripts.
- Standard CI runs the portable static smoke path. Separate manually dispatched workflows exist for a provisioned model runner and a release build that injects updater signing material from GitHub secrets.

The following external validation remains required:

1. Provision a self-hosted Apple Silicon runner with a compatible model and execute `Model-Backed Smoke`.
2. Create the GitHub `release` environment, set its `CONTEXTURA_UPDATER_PUBKEY`, `TAURI_SIGNING_PRIVATE_KEY`, and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` secrets, then validate an update from a staged release.
3. Run the packaged app and complete the manual GUI smoke pass with Screen Recording permission.

## Immediate Blockers

| Priority | Finding                                                | Evidence                                                                                                                  | Recommended next step                                                                                 |
| -------- | ------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------- |
| P0       | The Rust suite was not deterministic on battery power. | The runtime settings test inherited host power state and expected `275ms` while receiving the `1.2s` battery override.    | Completed: tests now choose AC or battery power explicitly and the full suite passes.                 |
| P0       | A packaged release needs authenticated updater config. | The checked-in config intentionally has no public key; release material is supplied only from GitHub environment secrets. | Workflow implemented. Set release secrets and validate a staged update.                               |
| P0       | The Tauri webview had no Content Security Policy.      | The previous config set `app.security.csp` to `null`; the Wizard contained an inline script.                              | Completed in code: restrictive CSP and same-origin Wizard script. Validate the packaged app manually. |

## Reliability And Delivery

| Priority | Finding                                               | Evidence                                                                                                         | Recommended next step                                                                                                                    |
| -------- | ----------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- |
| P1       | Automated CI did not cover portable smoke validation. | CI previously ran formatting, clippy, unit tests, release `cargo check`, and dependency audit only.              | Completed: CI now runs `smoke-wire-to-wire.sh --quick --static-only`; model-backed smoke is a provisioned, manually dispatched workflow. |
| P1       | The smoke harness had no model-independent mode.      | The original script always performed real translation requests.                                                  | Completed: `--static-only` validates Rust gates and corpus fixtures without a model; the default remains model-backed.                   |
| P1       | Manual end-to-end verification remains outstanding.   | The app requires Screen Recording permission, real Japanese screen content, and a local decoder-only GGUF model. | Perform and record a GUI smoke pass covering capture, force scan, overlay placement, app-switch invalidation, and watchdog recovery.     |

## Documentation Maintenance

| Priority | Finding                                                                   | Evidence                                                                                           | Recommended next step                                                                                          |
| -------- | ------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| P1       | Test status in the docs had become stale.                                 | `SPEC.md` and `TEST.md` claimed 135 passing tests, but the reviewed run failed as described above. | Update status only with dated command output; avoid unqualified "passes" claims until the regression is fixed. |
| P1       | The documented default debounce was inconsistent with the implementation. | `settings.rs` defaults `debounce_ms` to `150`, while multiple docs said `200ms`.                   | Treat `settings.rs` as the source for the default and document the battery override separately.                |
| P2       | The README previously implied that no remote CI pipeline existed.         | `.github/workflows/ci.yml` defines GitHub Actions checks on push and pull requests.                | Keep the README in sync with the workflow and state its current coverage limits.                               |

## Verification Snapshot

- `./scripts/smoke-wire-to-wire.sh --quick --static-only`: passed on 2026-08-05, including 136 unit tests, 2 CLI integration tests, and 9 fixture pairs.
- The model-backed smoke harness, staged updater validation, and GUI smoke pass remain unexecuted because they require a provisioned model runner, release secrets, or Screen Recording access.

## Completion Criteria

Before calling the project release-ready:

1. The Rust test suite, clippy, and formatting checks pass on a clean checkout.
2. A signed updater release is validated from a staged channel.
3. The packaged application runs with a restrictive CSP.
4. The live CLI corpus probe and manual GUI smoke pass have recorded successful evidence.
