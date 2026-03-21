# What's New in v2.2.3

This patch is narrowly focused on the installer handoff. The writing surface stays the same, while the launch path after install becomes much more reliable for real first-run users.

## Highlights

- fixed installer menu option `1` when the installer itself was started with `curl | bash`
  - the same-terminal handoff no longer drops the TUI into a broken input-reader state
- the installer now launches the blue-screen workspace under a clean PTY for that handoff path
- release QA now exercises the real piped-launch + `Launch here now` flow
  - not just a version check
  - it verifies the TUI actually initializes once before exiting the smoke harness

## Why this matters

BlueScreen Journal already had the core editor, vault, sync, backup, and recovery shape.

The weak point was the post-install launch edge:

- the installer could successfully install the app
- then fail right at the point where a new user tried to launch it from the menu
- and our older smoke checks were too shallow to catch that exact path

This release closes that gap without changing the nostalgic writing loop.

## Find it in the product

- installer menu option `1`
- installer `--debug`
- `bsj guide whatsnew`
- `docs/RELEASE_CERTIFICATION.md`
