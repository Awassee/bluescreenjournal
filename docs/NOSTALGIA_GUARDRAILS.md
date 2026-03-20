# Nostalgia Guardrails

BlueScreen Journal should feel like a dedicated blue-screen writing appliance, not a general note app with a retro skin.

This document is a release gate for product, UI, UX, docs, and engineering changes.

## Product promise

- Launch should lead directly to the editor or unlock prompt, not a dashboard-first workflow.
- Local passphrase-only use remains a first-class option.
- Sync, AI, cloud connectors, and admin tools stay optional layers around the writing flow.
- The app stays local-first and encrypted-first.

## Visual contract

- Royal-blue screen, white monospaced text, and restrained DOS-era presentation.
- Stable header, visible menu bar, and always-visible command strip.
- Center the experience around a classic `80x25` workspace, even on larger terminals.
- Compact layouts may adapt, but they should still look intentional and unmistakably `bsj`.
- Too-small terminals must show an actionable warning instead of clipped or corrupted UI.

## Interaction contract

- Writing comes first: unlock, type, save, move to the next entry.
- Keyboard-only use must remain complete.
- Menus are first-class. Every major feature must be reachable from `FILE`, `EDIT`, `SEARCH`, `GO`, `TOOLS`, `SETUP`, or `HELP`.
- Function keys, Alt bindings, and visible hints should make discovery possible without memorizing everything.
- CLI commands and env vars may mirror features, but they are fallback/admin surfaces, not the primary product UX.

## Trust contract

- No plaintext journal content on disk, in sync targets, or in caches that outlive the session.
- Lock state, verify state, save state, and recovery state should stay visible and understandable.
- Destructive or high-risk actions need clear confirmation and plain language.
- New features must not weaken the calm, focused feel of the editor.

## Release gate checklist

Before shipping a feature or UI change, verify:

- The main writing screen still shows the title, date, entry number, menus, and command strip.
- `Esc` menus, function keys, and menu navigation remain discoverable from the live screen.
- The feature works inside the canonical `80x25` mental model and a compact terminal layout.
- Small-terminal fallback remains readable and tells the user what to do next.
- The feature is available from in-product menus if it matters in normal use.
- Docs and help text describe the menu path, not just CLI flags.
- Render or regression tests were added when the visible flow changed.

## Automation expectations

- TUI-visible changes should add or update tests in `src/tui/app.rs`.
- Core editor/help/small-terminal screens should stay covered by `./scripts/check-tui-snapshots.sh`.
- Installer and release-path regressions should be covered in `scripts/smoke-release-install.sh` or `scripts/qa-gate.sh`.
- User-reported UX bugs should get a dedicated regression test before the fix ships.
