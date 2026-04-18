# PRODUCTION.md — Road to 10/10

**Document Version:** 2.0.0
**Related Spec:** SPEC.md v1.2.0 · TODO.md (Phase 0–8)
**Last Updated:** 2026-04-18

---

## Overview

This document tracks all production-readiness considerations beyond the core translation pipeline. It serves as a final polish checklist before public beta or initial release.

**Status Key:**

- ✅ **In Spec + TODO** — fully specified and has concrete implementation tasks
- 🔶 **Partial** — mentioned in spec but lacks full TODO tasks; needs attention before shipping
- 📋 **v1.1 Backlog** — deliberately deferred; not a v1.0 blocker

---

## Summary Table

| Consideration                      | Status                                 | Effort | Priority |
| ---------------------------------- | -------------------------------------- | ------ | -------- |
| Resumable Downloads                | ✅ In Spec + TODO (Phase 3.1)          | Medium | v1.0     |
| User Feedback During Translation   | ✅ In Spec + TODO (Phase 5.3)          | Small  | v1.0     |
| Silent Background Updates          | ✅ In Spec + TODO (Phase 6.5)          | Small  | v1.0     |
| Low-Memory System Guard            | ✅ In Spec + TODO (Phase 3.3)          | Small  | v1.0     |
| Privacy Transparency               | ✅ In Spec + TODO (Phase 3.1 Screen 4) | Small  | v1.0     |
| Minimal Settings File              | ✅ In Spec + TODO (Phase 0.6)          | Small  | v1.0     |
| In-App Help                        | ✅ In Spec + TODO (Phase 6.4)          | Small  | v1.0     |
| E2E Test Suite                     | ✅ In Spec + TODO (Phase 7.3)          | Medium | v1.0     |
| Display Hot-Plug Handling          | ✅ In Spec + TODO (Phase 1.1)          | Medium | v1.0     |
| Onboarding Polish & Error Recovery | ✅ In Spec + TODO (Phase 3.1)          | Medium | v1.0     |
| Accessibility & Localization       | 📋 v1.1 Backlog                        | Medium | v1.1     |
| Tab-Level Context Isolation        | 📋 v1.1 Backlog                        | Medium | v1.1     |

---

## 1. Onboarding Polish & Error Recovery ✅

**Now specified in:** SPEC.md §6.1, TODO.md Phase 3.1

All three original gaps are closed:

- **Resumable downloads:** HTTP Range requests + `.part` sidecar file; resumes on restart
- **Verification failure UI:** SHA256 mismatch deletes file and shows retry dialog
- **Background download:** Wizard close continues download in background; tray shows progress with cancel

No further action needed for v1.0.

---

## 2. Accessibility & Localization 📋

**Status:** Deferred to v1.1. In TODO Backlog.

The v1.0 app has English-only UI. This is acceptable for initial release given the target audience. For v1.1:

- Localization infrastructure: wrap UI strings in a `tr!()` macro or JSON key-value store; ship English + Japanese translations
- VoiceOver: `aria-label` on overlay boxes, `role="status"` on overlay container
- Original text toggle: tray option to show Japanese alongside translation (field already in IPC payload)

---

## 3. Silent Background Updates ✅

**Now specified in:** SPEC.md §12, TODO.md Phase 6.5

`tauri-plugin-updater` with GitHub Releases feed. Silent check on startup. Non-intrusive tray notification. User always opts in.

No further action needed for v1.0.

---

## 4. User Feedback During Long Operations ✅

**Now specified in:** SPEC.md §5.9, TODO.md Phase 5.3

`"translation-started"` event triggers a subtle bottom-right spinner (_"Translating…"_, opacity 0.6, no pointer events). Dismissed when `"translation-update"` or `"translation-clear"` arrives.

No further action needed for v1.0.

---

## 5. Display Hot-Plug Handling ✅

**Now specified in:** SPEC.md §5.1, TODO.md Phase 1.1

`CGDisplayRegisterReconfigurationCallback` handles display add/remove. On remove: stop SCStream, drop channel, close Tauri window. On add: create new stream and overlay window.

No further action needed for v1.0.

---

## 6. Minimal Settings File ✅

**Now specified in:** SPEC.md §7, TODO.md Phase 0.6

`settings.json` at `~/Library/Application Support/jp-translate/settings.json`. Created on first run with all defaults. Exposes: `debounce_ms`, `motion_threshold`, `pixel_diff_threshold`, `capture_fps`, `edge_inset_percent`, `furigana_suppression`, `show_original_text`, `context_memory_size`, `active_model`. Tray menu item reveals it in Finder.

No further action needed for v1.0.

---

## 7. E2E Test Suite ✅

**Now specified in:** SPEC.md §8, TODO.md Phase 7.3

`--debug-cli --test-suite <dir>` mode. Test corpus of PNG screenshots with `.expected.json` companions. Asserts OCR substrings + translation similarity. Exits `0`/`1` for CI integration. GitHub Actions to run on every commit.

**Action before shipping:** Curate the test corpus (at least 10 PNGs including 2 vertical-text and 1 furigana-heavy). This requires real Japanese screenshots — gather these during Phase 2 testing.

---

## 8. Privacy & Data Collection Transparency ✅

**Now specified in:** SPEC.md §14, TODO.md Phase 3.1 (Screen 4)

Onboarding Screen 4 lists all three network request categories explicitly. Opt-in Sentry checkbox with GitHub privacy policy link. _"The app never sends screen contents anywhere."_ Preference is saved and changeable via tray.

No further action needed for v1.0.

---

## 9. Graceful Degradation on Low-Memory Systems ✅

**Now specified in:** SPEC.md §5.4 (RAM Gate), TODO.md Phase 3.3

`sysctl hw.memsize` at startup. If total RAM < 12GB: Quality Mode fully disabled, greyed out in tray with explanatory tooltip, `Cmd+Shift+G` is a no-op.

No further action needed for v1.0.

---

## 10. In-App Help ✅

**Now specified in:** SPEC.md §10, TODO.md Phase 6.4

Bundled `help.html` opened from tray → "Help". Covers: permission setup, all 5 hotkeys, model switching, context memory, FAQ with 4 common questions.

No further action needed for v1.0.

---

## 11. Tab-Level Context Isolation 📋

**Status:** Deferred to v1.1. Noted in TODO Backlog.

By design, context memory is scoped to the active _application_ (bundle ID), not the active browser tab. Switching Safari tabs does NOT clear context — this is intentional for multi-tab Japanese reading sessions.

For users who want per-tab isolation (e.g., comparing two different Japanese texts side by side in the same browser), v1.1 could add optional tab-level tracking via accessibility APIs or browser extension bridge. This is complex and not worth v1.0 scope.

---

## Remaining Action Items Before v1.0 Ship

These are the only outstanding tasks that don't yet have a concrete "done" state:

1. **Curate E2E test corpus** (Phase 7.3) — gather 10+ Japanese screenshots during Phase 2 testing; write `.expected.json` for each
2. **Obtain real SHA256 hashes** for model files and populate `manifest.json` template in SPEC §6.2
3. **Register Apple Developer account** if not already done (Phase 0.1 prerequisite)
4. **Host privacy policy** on project GitHub before onboarding wizard goes live (referenced on Screen 4)
5. **Set up GitHub Releases** JSON feed for auto-updater (Phase 6.5)

All other production considerations are fully specified and have corresponding TODO tasks.

---

## Estimated Remaining Effort

| Phase      | Remaining Work                | Estimate        |
| ---------- | ----------------------------- | --------------- |
| Phases 0–2 | Scaffold, capture, OCR        | 2–3 weeks       |
| Phase 3    | Translation + all sub-systems | 3–4 weeks       |
| Phases 4–5 | Styling + frontend            | 1 week          |
| Phase 6    | Polish, wizard, hotkeys       | 1–2 weeks       |
| Phase 7    | Testing + hardening           | 1 week          |
| Phase 8    | Build + distribution          | 3–5 days        |
| **Total**  |                               | **~9–12 weeks** |

Estimates assume you're learning Rust as you go. With prior Rust experience, knock 30–40% off.
