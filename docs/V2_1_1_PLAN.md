# v2.1.1 Patch List

This patch list tracks the first follow-up work after the `v2.1.0` release.

## Live-release findings carried forward

1. Keep the public installer smoke step mandatory.
   - The one-line installer is a release-critical surface, not a nice-to-have.

2. Watch for GitHub Raw propagation lag on `main`.
   - Tag/commit URLs can reflect pushed installer changes before the `main` raw URL catches up.

3. Keep asset-selection logic explicit for macOS release downloads.
   - The installer should continue choosing the best available published asset for the current Mac.

## Candidate patch work

1. Add a second-environment public install check.
   - Acceptance:
   - one fresh-machine install is recorded outside the primary workstation
   - findings are documented in this plan or closed

2. Improve release diagnostics around selected asset and CDN propagation.
   - Acceptance:
   - installer doctor shows which asset it would fetch
   - release runbook explicitly checks both tag and `main` raw installer URLs

3. Expand public installer smoke assertions.
   - Acceptance:
   - script validates selected asset, version, PATH integration, and bundled docs/man page presence

4. Review Intel/universal packaging coverage.
   - Acceptance:
   - distribution docs accurately describe published macOS assets
   - release process documents whether the current release is `aarch64`, `x86_64`, or universal
