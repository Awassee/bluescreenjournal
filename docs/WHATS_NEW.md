# What's New in v2.2.1

This patch keeps the writing surface stable and cleans up the trust-and-onboarding edges around it.

## Highlights

- `TOOLS -> Status Dashboard` now opens the `Trust Dashboard` drill-down instead of a static report.
  - from one place you can open verify, integrity details, backup actions, sync status, cloud recovery, doctor, and the short trust snapshot
- `HELP -> What's New` now explains the latest shipped changes without sending you to GitHub first
  - CLI mirror: `bsj guide whatsnew`
  - installer guidance now points to the same short summary
- first-save guidance now stays visible a little longer
  - after your first save, the footer keeps the next-step language obvious:
    - revise the saved page
    - use `**save**` for the next same-day entry
    - use `Alt+N` for the next blank day
- the installer post-install summary is shorter and more direct
  - less manual dump
  - more “what should I do next?”

## Why this matters

BlueScreen Journal already had the core writing loop.

The friction was around confidence:

- users could save, but not always tell what to do immediately after
- the trust dashboard had the right information, but not the right shape
- release highlights lived in GitHub instead of inside the product

This release closes those gaps while preserving the nostalgic blue-screen flow.

## Find it in the product

- `HELP -> What's New`
- `HELP -> First 2 Minutes Cheat Sheet`
- `TOOLS -> Status Dashboard`
- installer menu option `4` for the cheat sheet, plus `bsj guide whatsnew` after install
