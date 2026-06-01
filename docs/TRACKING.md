# Roadmap Tracking Workflow

Last updated: 2026-05-29

## How We Track Work

Use docs/ROADMAP.md as the single source for priority and status.
Use docs/ARCHITECTURE.md for system boundaries and design decisions.
Keep docs/ as canonical; avoid maintaining duplicate planning copies outside docs/.

## Update Rules

1. Before starting a feature, add or update the corresponding roadmap section with scope and exit criteria.
2. While implementing, keep status labels current (In Progress, Next Up, Done).
3. When behavior or data model changes, update architecture notes in docs/ARCHITECTURE.md.
4. After completing a feature, add a short changelog entry to docs/ROADMAP.md.
5. After UX/runtime behavior changes, verify docs/ARCHITECTURE.md recovery/error-handling notes remain accurate.

## Doc Audit Checklist

Run this pass at least once per feature branch before PR:

1. ROADMAP status, scope, and change log match actual branch state.
2. ARCHITECTURE runtime flow reflects current startup/recovery behavior.
3. migrated-Torquebridge docs remain reference-only and are not edited for active planning.

## Suggested Status Labels

- Planned
- In Progress
- Blocked
- Done

## Current Active Item

- Per-Effect Calibration (feature/per-effect-calibration)

## Historical Documents

Legacy Torquebridge planning and analysis notes are stored in docs/migrated-Torquebridge for reference only.
