# Torquebridge Architecture

Last updated: 2026-05-30

## Runtime Overview

Torquebridge is a compatibility bridge between game FFB packets and the physical FFBeast wheel.

Flow:

1. vJoy callback receives game force updates.
2. Packet parser and effect engine translate updates into wheel commands.
3. DirectInput backend applies persistent effect objects to the physical wheel.
4. Optional WinMM steering state feeds condition-effect translation.
5. Profile Studio edits profile data and launches/stops bridge runtime.

## Main Subsystems

### Frontend Input/FFB Ingress

- src/frontends/forza_vjoy.rs
- src/ffb_packet.rs
- src/vjoy/ffi.rs

Responsibilities:

- Receive and decode incoming force packets.
- Normalize packet events before translation.

### Translation Engine

- src/effect_engine.rs
- src/core/domain.rs

Responsibilities:

- Map packet updates to device-agnostic wheel commands.
- Apply calibration and profile-driven behavior.
- Apply optional inferred traction-loss release shaping from steering/force heuristics.
- Apply optional inferred dynamics shaping for torque-steer, brake imbalance pull, understeer scrub, rear-lightness release, curb asymmetry, and snap-oversteer catch.

### Physical Output Backend

- src/backends/directinput.rs
- src/backends/ffbeast_direct.rs (experimental/fallback)

Responsibilities:

- Manage effect object lifecycle on the physical target wheel.
- Apply translated commands and device-control actions.

### Profile and Persistence

- src/profile.rs
- profiles/\*.json

Responsibilities:

- Persist force configuration, runtime mapping, and profile metadata.
- Support safe defaults for backward compatibility.
- Persist and reuse last-used profile selection for startup.

### UI and Runtime Control

- ui/profile_editor.slint
- src/ui.rs

Responsibilities:

- Provide editing surface for profile parameters.
- Manage launch/stop lifecycle and status reporting.
- Persist last-used profile selection.
- Keep launch status/errors copyable from the header status field.
- Surface runtime routing source and launch preflight state directly in Runtime view.
- Provide in-studio A/B profile diff for routing + core force/calibration values.
- Keep runtime flow plug-and-play: select steering input, save profile, launch.
- Keep diagnostics/export/replay workflows optional and isolated in Diagnostics.
- Track diagnostics workflow progress and last export destinations in-session.
- Emit a startup snapshot event at launch so diagnostics and telemetry retain profile-routing context.
- Expose Expirimental tuning controls for traction-loss release behavior.

### Runtime Process + Telemetry

- src/main.rs
- src/diagnostics.rs

Responsibilities:

- Run bridge loop, hot reload profile settings, and emit diagnostics snapshots.
- Support session export/replay for debugging.
- Surface latest startup snapshot metadata alongside live telemetry for runtime triage.

## Architectural Direction

1. Profile-first operation: profile should be sufficient to launch bridge behavior.
2. Clear separation: packet parsing, translation rules, and hardware backend remain independent.
3. Backward compatibility: new profile fields must default safely.
4. Diagnosability: startup and runtime failures must be explicit and copyable.
5. Graceful recovery: missing embedded runtime controllers should try deterministic recovery paths before failing.
6. Scope discipline: Torquebridge remains a translation layer; wheel setup responsibilities stay with FFBeast tooling.

## Current Design Debt

1. Legacy fallback pathways still exist for profiles without embedded runtime controllers.
2. Runtime routing warning states are visible in preflight text but could use stronger visual emphasis.
3. A/B compare is grouped and mode-aware, but lacks richer visual widgets (tables/charts).
4. Diagnostics/telemetry copy-export UX can be improved beyond current workflow hints and export-location state.

## Implemented Recovery Paths

When a profile has no embedded runtime controllers:

1. Try controllers from another profile with matching steering device.
2. Fall back to discovered/explicit config path when available.
3. Emit explicit launch failure if no valid controller source can be found.

Runtime panel now surfaces both route status and source classification so fallback behavior is visible before launch.

## References

Historical and analysis documents:

- docs/migrated-Torquebridge/FFBEAST_PHASE2_ROADMAP.md
- docs/migrated-Torquebridge/Torquebridge_RS_ARCHITECTURE.md
- docs/migrated-Torquebridge/FFBEAST_RUST_REIMPLEMENTATION_PLAN.md
