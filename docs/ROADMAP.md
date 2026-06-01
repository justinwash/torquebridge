# Torquebridge Roadmap

Last updated: 2026-05-30
Owner: Torquebridge

## Purpose

Track product work in this repository with Torquebridge-owned scope and priorities.

## Current Release Focus

1. Finish per-effect calibration as a complete feature (data model, engine wiring, editor controls, telemetry visibility).
2. Harden profile-first startup/launch so profiles are self-contained and resilient when legacy fields are missing.
3. Improve bridge observability, compare tooling, and diagnostics export for repeatable tuning and bug reports.

## Recently Completed

- Profile-first UI startup now supports launching without requiring --config when using an existing profile.
- Last-used profile persistence is enabled and reused at startup.
- Status/error message text in Profile Studio is selectable/copyable.
- Runtime-controller recovery now attempts selected-device profile matching before config fallback.

## In Progress

## Per-Effect Calibration (branch: feature/per-effect-calibration)

Scope:

- Add per-effect gain controls with backward-compatible defaults.
- Keep profile-first launch behavior resilient when routing fields are missing.
- Make routing source and launch readiness explicit in the Runtime panel.
- Add lightweight A/B profile compare and plug-and-play runtime flow.

Exit criteria:

- Per-effect calibration fields persist/load with defaults for older profiles.
- Effect translation honors per-effect gains for constant/periodic/spring/damper.
- Runtime panel shows routing source and preflight status.
- Profile Studio supports in-app A/B compare for core force + routing values.
- Runtime setup is direct: select steering input, save profile, launch.

Progress this branch:

- Added Runtime routing/preflight inspector block in Profile Studio runtime view.
- Added routing source detail line (embedded, matched profile, fallback config, or missing).
- Added one-click debug bundle export in diagnostics view (profile copy, diagnostics log, event tail, summary).
- Added Compare tab with profile A/B diff report for runtime + force/calibration fields.
- Added diagnostics workflow hints plus copyable last export locations for session exports and debug bundles.
- Added startup snapshot diagnostics event on launch with profile, routing source, and preflight metadata.
- Surfaced startup snapshot context inside telemetry runtime detail for faster triage.
- Added compare mode toggle (changed-only vs all fields) and grouped diff sections.
- Added Expirimental tab controls for inferred traction-loss release modeling (thresholds, release/recovery shaping, per-effect apply toggles).
- Added translation-engine support for configurable traction-release force scaling on constant/spring/damper effects.
- Added inferred dynamics pack (torque-steer, brake-imbalance pull, understeer scrub, rear-lightness release, curb asymmetry, snap-oversteer catch) behind experimental profile controls.
- Removed first-run setup wizard and runtime quickstart controls to keep runtime flow plug-and-play and translation-layer focused.
- Hardened preflight checks to block launch when steering assignment is missing and warn clearly when routing depends on fallback sources.

## Next Up

### 1) Profile Portability Hardening

Goals:

- Ensure runtime controllers are always embedded after normal save/start flow.
- Add explicit warning text when launch depends on fallback config source.
- Add deterministic profile validation checks before launch.

### 2) Diagnostics Workflow

Goals:

- Expand copy/export beyond header status text into diagnostics/telemetry panels.
- Keep session export/replay fast for tuning comparisons.
- Add concise startup snapshot fields for profile + routing source classification.

Status:

- Startup snapshot fields are now emitted at launch and surfaced in diagnostics + telemetry; remaining work is mostly UX polish.

### 3) Runtime Flow Simplification

Goals:

- Keep runtime setup minimal and translation-layer focused.
- Avoid replacing external wheel setup responsibilities.
- Keep diagnostics/replay workflows optional for testing-heavy use cases.

Status:

- Completed: wizard/quickstart controls removed from Runtime panel. Flow is now select device -> save profile -> launch bridge, with diagnostics tooling left optional.

### 4) Native Frontend Research Gate

Goals:

- Keep vJoy compatibility path as baseline.
- Define concrete acceptance criteria before any vJoy-removal work.

Readiness gate before vJoy-removal branch can begin:

- Match packet ingestion coverage for constant, periodic, and condition effects against current vJoy path.
- Provide deterministic fallback path when native ingress is unavailable.
- Demonstrate no regression in diagnostics telemetry fields used by Profile Studio.
- Run side-by-side replay comparison showing command parity within documented tolerances.
- Keep existing profile schema and editor workflow unchanged for users.

## Not Current Priority

- Removing vJoy in the short term.
- Deep hardware protocol research as the primary runtime path.
- Broad force-feel retuning before per-effect calibration is finished.

## Change Log

- 2026-05-29: Established Torquebridge-owned roadmap and documentation workflow.
- 2026-05-29: Updated roadmap to reflect profile-first startup, last-used profile persistence, and current diagnostics/runtime-controller recovery behavior.
- 2026-05-31: Added experimental traction-loss release feature with profile-backed tuning controls in Profile Studio.
- 2026-05-31: Added experimental inferred-dynamics heuristics with profile-backed defaults and starter tune in baseline profile.
- 2026-05-30: Simplified runtime UX to plug-and-play by removing startup wizard/quickstart controls and strengthening preflight readiness messaging.
