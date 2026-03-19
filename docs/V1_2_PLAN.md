# BlueScreen Journal v1.2.0 Plan

This plan is execution-first and release-gated.
Nothing is considered done until the quality gates pass and `v1.2.0` is published.

## Release Goal

Ship `v1.2.0` as a quality and onboarding release that improves:

- first-run success (users understand exactly how to start and continue)
- trust visibility (clear, quick health/status view)
- release reliability (deterministic CI gate for critical UX paths)

## Scope

In scope for `v1.2.0`:

1. First-Run Tour surface in-app
2. Journal Health surface in-app
3. Stability gate for critical UX tests
4. Version/docs/release update to `v1.2.0`

Out of scope for `v1.2.0`:

- net-new sync backends or crypto format changes
- major editor model rewrites
- data model migration

## Milestones

### M1: Plan + Issue Decomposition

Status: `Complete`

Issues:

- `V12-001` Create milestone plan with ordered execution and acceptance criteria.
- `V12-002` Define release gates (local + CI) required for ship.

Acceptance criteria:

- plan doc exists in repo
- every issue has explicit done criteria and test mapping

### M2: First-Run Experience Upgrade

Status: `Complete`

Issues:

- `V12-101` Add a menu-accessible First-Run Tour with a 2-minute flow.
- `V12-102` Ensure first-run guidance points to save + quick-save + next-entry behavior.
- `V12-103` Add tests for First-Run Tour discoverability and content.

Acceptance criteria:

- tour is reachable through menus without CLI arguments
- tour copy includes:
  - write immediately
  - save (`F2` / `Ctrl+S`)
  - quick-save (`**save** + Enter`)
  - open menus (`Esc` / `Ctrl+O`)
- tests verify menu presence and overlay rendering

### M3: Trust Surface (Journal Health)

Status: `Complete`

Issues:

- `V12-201` Add a concise Journal Health info surface under `TOOLS`.
- `V12-202` Include lock, save, integrity, backups, sync target, and conflict signals.
- `V12-203` Add tests for Journal Health menu visibility and content.

Acceptance criteria:

- Journal Health is reachable via in-product menu
- surface includes minimum fields:
  - vault lock state
  - date + entry number
  - latest save/autosave signal
  - integrity summary
  - backup count
  - conflict count
  - sync target summary
- tests verify action routing and key labels

### M4: Stability Gate Hardening

Status: `Complete`

Issues:

- `V12-301` Add a repeat-run stability script for critical UX tests.
- `V12-302` Include stability script in `qa-gate`.
- `V12-303` Enforce stability script in CI and release workflows.

Acceptance criteria:

- script runs targeted tests repeatedly with fail-fast behavior
- `qa-gate` fails if the stability script fails
- GitHub CI and release workflows call the stability gate before packaging

### M5: Release Packaging + Publish

Status: `In Progress`

Issues:

- `V12-401` Bump version/references to `v1.2.0`.
- `V12-402` Add release notes file `docs/releases/v1.2.0.md`.
- `V12-403` Run full validation and publish tag/release.

Acceptance criteria:

- local gates pass:
  - `cargo fmt --all`
  - `cargo clippy --all-targets -- -D warnings`
  - `cargo test --all-targets`
  - `./scripts/qa-gate.sh`
- release workflow succeeds on tag push
- GitHub release exists with expected assets

## Quality Gates (Ship Blockers)

`v1.2.0` ship is blocked unless all are green:

1. formatting + lint + tests pass locally
2. `qa-gate` passes locally
3. release workflow passes on tag push
4. release assets are published and downloadable

## Execution Log

- `[x]` M1 complete
- `[x]` M2 complete
- `[x]` M3 complete
- `[x]` M4 complete
- `[ ]` M5 complete
