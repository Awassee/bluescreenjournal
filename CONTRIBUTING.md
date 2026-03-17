# Contributing

## Standard for contributions

Changes should preserve the product direction:

- local-first encrypted journal
- nostalgic DOS-style TUI feel
- keyboard-first interaction
- no silent plaintext persistence in the vault

## Before opening a pull request

Run:

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
```

For packaging or installer changes, also run:

```bash
./scripts/package-release.sh
./scripts/smoke-release-install.sh
```

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
