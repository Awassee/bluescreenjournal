# Contributing

## Standard for contributions

Changes should preserve the product direction:

- local-first encrypted journal
- nostalgic DOS-style TUI feel
- keyboard-first interaction
- no silent plaintext persistence in the vault

See [docs/NOSTALGIA_GUARDRAILS.md](docs/NOSTALGIA_GUARDRAILS.md) for the explicit release gate.

## Nostalgia release gate

Before merging a visible product change, confirm:

- the main screen still reads as BlueScreen Journal, not a general note app
- header, menus, and footer command strip stay visible and useful
- primary features remain menu-first and keyboard-complete
- CLI or env setup remains optional fallback, not the main path
- small-terminal behavior stays actionable instead of glitchy
- changed screens have regression coverage

## Before opening a pull request

Run:

```bash
./scripts/qa-gate.sh
```

This includes:

```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
./scripts/check-tui-snapshots.sh
cargo test --all-targets
./scripts/package-release.sh
./scripts/smoke-release-install.sh
./scripts/installer-action-matrix.sh
```

`./scripts/check-tui-snapshots.sh` also writes a browsable nostalgia preview at
`artifacts/tui-snapshots/index.html` so UI regressions can be reviewed without
replaying the TUI locally.

Shortcut via `just`:

```bash
just qa
```

## Regression policy

- Every user-reported bug must get a dedicated regression test in the same PR.
- UX flow bugs should be covered by `src/tui/app.rs` tests under the `tui::app::tests` module.
- Installer/update regressions should be guarded in `scripts/smoke-release-install.sh`, `scripts/installer-action-matrix.sh`, or `scripts/qa-gate.sh`.

## Good pull requests

A good PR explains:

- the user problem
- the product reason for the change
- why the solution fits bsj
- what was tested
- what risks remain

## Docs changes are product work

If the feature surface changes, update the relevant docs and help text in the same PR.

## Security-sensitive changes

If a change touches encryption, sync, backups, verification, or anything that may affect plaintext leakage, raise the review bar and document the reasoning clearly.
