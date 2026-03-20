# v2.2.1 Patch Plan

This patch line follows the `v2.2.0` release.

The goal is to build on the onboarding and release-trust work without losing the writing-first core.

## Candidate patch work

1. Add a trust dashboard drill-down flow.
   - Acceptance:
   - from one entry point, users can open verify, backup, sync, and recovery detail screens
   - the current state of the vault is understandable at a glance

2. Add an in-app `What changed in this release` surface.
   - Acceptance:
   - `HELP` can explain the newest shipped changes without requiring GitHub
   - installer and docs can point to the same short summary

3. Tighten onboarding after the first save.
   - Acceptance:
   - save, next same-day entry, next blank day, and old-entry access stay obvious until the first confident session
   - the guidance fades once it is no longer needed

4. Run and record one second-environment human certification outside the primary workstation.
   - Acceptance:
   - one release note or certification record confirms a real external-machine or second-account install/launch check

5. Trim installer guide excerpts further.
   - Acceptance:
   - post-install guide output feels like a short assist, not a manual dump

6. Keep visual nostalgia review explicit in release automation.
   - Acceptance:
   - CI and release summaries continue to point reviewers to the snapshot artifact preview
