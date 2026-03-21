# Release Certification

Stable BlueScreen Journal releases should feel trustworthy before they feel clever.

This runbook defines the minimum certification pass for a public release.

## Required gates

1. `cargo fmt --all`
2. `cargo clippy --all-targets -- -D warnings`
3. `cargo test --all-targets`
4. `./scripts/check-tui-snapshots.sh`
5. `./scripts/package-release.sh --universal`
6. `./scripts/smoke-release-install.sh --archive dist/bsj-<version>-universal-apple-darwin.tar.gz`
7. `./scripts/smoke-public-install.sh --ref v<version> --version v<version>`
8. `./scripts/certify-release.sh --ref v<version> --version v<version> --report docs/certification/v<version>.md`

## What certification means

`certify-release.sh` is the clean-account release check.

It:

- runs the public GitHub installer against a chosen ref or tag
- installs into a clean temp `HOME` and isolated prefix
- verifies the installed binary version
- checks that `bsj --help` exposes the first-run guides
- checks that `bsj guide cheatsheet` renders the short first-use flow
- checks that `bsj guide whatsnew` renders the current release highlights
- checks that `bsj guide quickstart` renders the longer walkthrough
- writes a Markdown report that can be committed alongside release notes

This is the minimum required certification for a stable tag.

A second physical Mac or second user account check is still recommended for major releases, but the clean-account certification is the non-optional floor.

## Visual review

Nostalgia regressions are not always functional regressions.

Before cutting a stable tag:

- open the QA artifact `artifacts/tui-snapshots/index.html`
- compare the key screens, not just the test counts
- confirm the workspace still feels like a blue-screen DOS writing tool:
  - strong header
  - clear menus
  - readable footer
  - no clutter in the writing area
  - obvious first-run save/menu guidance

## Free disk guidance

Universal packaging and smoke validation need working room.

Keep at least `3-5 GiB` free before running the full release flow. Low disk space has already caused false-negative release runs during `lipo`, packaging, and smoke-install temp extraction.

## Release QA notes

If validation hits an environment-only issue, record it in the release notes under `Release QA Notes`.

Examples:

- local disk pressure during packaging
- CDN/raw GitHub propagation lag immediately after push
- shell profile oddities in a temporary test account

Do not hide these notes. They help distinguish product defects from release-environment noise.
