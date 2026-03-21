# What's New in v2.2.2

This patch focuses on release confidence. The writing surface stays the same, while the installer and release process get easier to trust when you revisit the product later.

## Highlights

- the installer now prints explicit state transitions
  - install/update, PATH repair, verification, and post-install handoff all show what the script is doing
- `./install.sh --debug` now exposes richer trace output for hard-to-reproduce install problems
  - no secrets are printed
- release QA now exercises more installer combinations
  - bundled install
  - public bootstrap install
  - existing-install update
  - repair-path, uninstall, and factory reset action flows
- a new maintenance baseline lives in the docs
  - it gives a wake-up checklist for coming back after months away
  - it records the exact scripts and checks to rerun before the next release

## Why this matters

BlueScreen Journal already had the core editor, vault, sync, backup, and recovery shape.

The weak point was release durability:

- installer hangs were harder to diagnose than they should have been
- some update combinations needed stronger regression coverage
- the “what do I do when I come back to this later?” story lived in memory instead of in the repo

This release hardens those edges without changing the nostalgic writing loop.

## Find it in the product

- installer `--debug`
- installer menu option `4` for the cheat sheet
- `bsj guide whatsnew`
- `docs/MAINTENANCE_BASELINE.md`
