# Maintenance Baseline

This document is the “wake this project back up in a few months” checklist.

The goal is simple: future maintenance should start from a known good baseline instead of re-discovering the release process by hand.

## Stable line

- current stable line: `v2.2.3`
- expected public entry point: `curl -fsSL https://raw.githubusercontent.com/Awassee/bluescreenjournal/main/install.sh | bash`
- intended packaged artifact: `dist/bsj-<version>-universal-apple-darwin.tar.gz`

## What is protected now

- packaged installer smoke:
  - `./scripts/smoke-release-install.sh`
- public bootstrap smoke:
  - `./scripts/smoke-public-install.sh`
- installer action matrix:
  - `./scripts/installer-action-matrix.sh`
- release certification:
  - `./scripts/certify-release.sh`
- nostalgia UI regressions:
  - `./scripts/check-tui-snapshots.sh`
- broader validation:
  - `./scripts/qa-gate.sh`

## Before touching the release line again

Run these in order:

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
./scripts/qa-gate.sh
./scripts/package-release.sh --universal
./scripts/smoke-release-install.sh --archive dist/bsj-<version>-universal-apple-darwin.tar.gz
./scripts/installer-action-matrix.sh --archive dist/bsj-<version>-universal-apple-darwin.tar.gz
./scripts/smoke-public-install.sh --ref main --version v<version>
```

## If the installer feels hung

Use:

```bash
./install.sh --debug
```

or:

```bash
curl -fsSL https://raw.githubusercontent.com/Awassee/bluescreenjournal/main/install.sh | bash -s -- --debug
```

Expected state lines now include:

- `State: copying bundled app files`
- `State: checking PATH integration`
- `State: verifying installed binary`
- `State: opening post-install options`
- `State: post-install summary is ready`

If the last line shown is a menu state, the installer is waiting for input on the current terminal.

## If disk space is tight

The heaviest local operation is universal packaging plus source-update smoke.

Useful cleanup:

```bash
cargo clean
find /var/folders -path '*/T/bsj-dist-smoke.*' -prune -exec rm -rf {} +
find /var/folders -path '*/T/bsj-installer-actions.*' -prune -exec rm -rf {} +
find /var/folders -path '*/T/bsj-public-smoke.*' -prune -exec rm -rf {} +
```

## Where release confidence lives

- release checklist: [RELEASE_CERTIFICATION.md](RELEASE_CERTIFICATION.md)
- distribution workflow: [DISTRIBUTION.md](DISTRIBUTION.md)
- current release notes: [releases/v2.2.3.md](releases/v2.2.3.md)
- public certification records: [`docs/certification/`](certification/)

## Recommended next re-entry point

If the project has been idle, start with:

1. `git pull`
2. `./scripts/qa-gate.sh`
3. `./scripts/smoke-public-install.sh --ref main --version v2.2.3`
4. read [WHATS_NEW.md](WHATS_NEW.md) and the latest release notes

That sequence is enough to tell you whether the stable line is still healthy before taking on new feature work.
