# BlueScreen Journal v1.3.0 Plan

This plan is scoped for a UX-first release with hard quality gates.
No milestone is complete until tests and release gates pass.

## Release Goal

Ship `v1.3.0` as the “daily writer confidence” release:

- faster start to writing
- clearer save/entry flow
- stronger menu discoverability
- fewer terminal-specific surprises

## Scope

In scope for `v1.3.0`:

1. writing flow simplification (new-entry and quick-save confidence)
2. menu usability improvements and command clarity
3. terminal compatibility polish (small sizes, key handling, redraw stability)
4. installer + onboarding continuity
5. stronger functional QA coverage for real user flows

Out of scope for `v1.3.0`:

- crypto format migrations
- new remote sync backends
- cloud AI feature expansion

## Milestones

### M1: Flow Baseline + UX Spec

Status: `Completed` (2026-03-19)

Issues:

- `V13-001` Define canonical “daily entry” flow and success metrics.
- `V13-002` Define canonical “open old entry” flow with higher friction than today flow.
- `V13-003` Publish in-repo UX acceptance checklist for these flows.

Acceptance criteria:

- flow specs are documented
- checklist maps to functional tests

### M2: Writing Flow Clarity

Status: `Completed` (2026-03-19)

Issues:

- `V13-101` Improve “save complete” feedback to remove ambiguity.
- `V13-102` Make “next clean same-day entry page” behavior explicit in UI copy.
- `V13-103` Improve date prominence and entry context in header.

Acceptance criteria:

- save state is obvious within one glance
- quick-save + next-entry behavior is clear and test-covered
- date/entry context is always visible and readable

### M3: Menu and Command Discoverability

Status: `Completed` (2026-03-19)

Issues:

- `V13-201` Improve top-menu labels and command naming consistency.
- `V13-202` Add context-sensitive menu hints for likely next action.
- `V13-203` Ensure all key workflows are reachable from menus without CLI memorization.

Acceptance criteria:

- users can complete primary flows via menus
- command labels are consistent and non-ambiguous
- new functional tests verify menu-driven completion paths

### M4: Terminal Robustness

Status: `Completed` (2026-03-19)

Issues:

- `V13-301` Harden redraw behavior across menu/open/return/edit transitions.
- `V13-302` Improve narrow-terminal and resize messaging/behavior.
- `V13-303` Extend regression tests for rendering artifacts and key routing edge cases.

Acceptance criteria:

- no visible stale glyph artifacts in tested transitions
- small terminal mode remains actionable and stable
- tests capture previously reported redraw bugs

### M5: Installer + Onboarding Continuity

Status: `Completed` (2026-03-19)

Issues:

- `V13-401` Refine installer finish UX into one clean launch-first action path.
- `V13-402` Ensure PATH setup remains automatic and verifiable.
- `V13-403` Keep in-app first-run guidance aligned with installer promises.

Acceptance criteria:

- installer output is concise and user-friendly
- post-install `bsj` resolution is validated in smoke tests
- first-run in-app guidance matches installer messaging

### M6: QA + Release

Status: `Completed` (2026-03-19)

Issues:

- `V13-501` Add human-like functional flow tests for daily write/review sessions.
- `V13-502` Run full gate (`fmt`, `clippy -D warnings`, `test`, `qa-gate`).
- `V13-503` Publish `v1.3.0` with release assets and release notes.

Acceptance criteria:

- quality gates pass locally and in CI
- release workflow succeeds
- release assets publish and smoke install passes

## Quality Gates

`v1.3.0` ship gate:

1. `cargo fmt --all --check`
2. `cargo clippy --all-targets -- -D warnings`
3. `cargo test --all-targets`
4. `./scripts/qa-gate.sh`
5. release workflow success on tag push

## Delivered in v1.3.0

1. Spellcheck engine with in-memory dictionary + suggestions.
2. `EDIT -> Spellcheck Entry` picker with one-click fixes.
3. `EDIT -> Next/Previous Misspelling` cursor navigation.
4. `EDIT -> Auto-Fix Common Typos` safe typo pass.
5. `EDIT -> Add Word At Cursor` session ignore dictionary.
6. CLI `bsj spellcheck` (`--date`, `--from/--to`, `--range`, `--json`, `--count-only`).
7. `FILE -> Save Receipt` confidence overlay after manual save.
8. `HELP -> Daily Flow Coach` in-app guidance for write/save/next flow.
9. Archive guard for deep backward date jumps (repeat to confirm).
10. New functional regression tests for spellcheck, save receipt, and archive guard.

## Delivered in v1.3.1 (feature-count correction pass)

`TOOLS -> Insights Center` now ships ten distinct report modules:

1. Momentum Snapshot report.
2. Save Readiness report.
3. Word Volume report.
4. Streak Tracker report.
5. Mood Mix report.
6. Tag Radar report.
7. People Radar report.
8. Project Radar report.
9. Gap Finder report.
10. Conflict & Backup Risk report.
