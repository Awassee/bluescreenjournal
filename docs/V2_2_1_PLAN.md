# v2.2.1 Release Plan

This release followed `v2.2.0`.

The goal was to build on the onboarding and release-trust work without losing the writing-first core.

## Shipped

1. Trust dashboard drill-down flow
   - `TOOLS -> Status Dashboard` now opens a picker-based trust surface
   - verify, backup, sync, recovery, doctor, and the short trust snapshot are reachable from one place

2. In-app `What changed in this release` surface
   - `HELP -> What's New`
   - `bsj guide whatsnew`
   - installer guidance points to the same short summary

3. Tighter onboarding after the first save
   - save, next same-day entry, next blank day, and old-entry access remain obvious immediately after the first successful save
   - the stronger guidance fades once the next writing action begins

4. Recorded clean-account release certification
   - stable release validation includes a public-install certification report committed with the release line

5. Trimmed installer guide excerpts
   - post-install output is shorter and more action-oriented

6. Kept visual nostalgia review explicit in release automation
   - CI and release summaries continue to point at the snapshot artifact preview

## Outcome

`v2.2.1` is a confidence patch:

- clearer trust surfaces
- clearer first-save flow
- clearer release communication

The writing surface stays intentionally narrow and nostalgic while the surrounding product edges become easier to trust.
