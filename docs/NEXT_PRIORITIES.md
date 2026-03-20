# Next Priorities

This is the current prioritized backlog after `v2.2.0`.

## Recently shipped in v2.2.0

- added a real first-run cheat sheet guide and wired it into CLI help, Help menus, installer output, and docs
- added clean-account release certification with a reusable script and commit-friendly report format
- surfaced nostalgia snapshot review directly in CI and release summaries, with browsable HTML artifact previews

## 20 major steps

1. Ship a trust dashboard drill-down flow.
   - Goal: from one place, users can understand vault health, verify, backup, sync, and recovery readiness.

2. Improve conflict merge ergonomics.
   - Goal: make competing heads understandable and resolvable without fear.

3. Add importers for plaintext journal formats into encrypted vault revisions.
   - Goal: let new users migrate from old diaries, Markdown journals, and text files.

4. Add a lightweight onboarding coach that disappears after first confident use.
   - Goal: teach save, next entry, next day, menus, calendar, and old-entry access without cluttering long-term use.

5. Strengthen search as a review tool.
   - Goal: saved searches, recurring prompts, and review workflows should feel instant and intentional.

6. Deepen spellcheck into a real writing assistant.
   - Goal: personal dictionary, session dictionary UX, review-before-save signals, and better typo explanations.

7. Add safer restore and recovery drills in-product.
   - Goal: backup and restore should feel tested, not theoretical.

8. Build a proper release runbook with explicit gates and failure classes.
   - Goal: distinguish product regressions from environment failures like disk pressure or CDN lag.

9. Improve cloud sync observability.
   - Goal: users should know which backend is active, what remote is selected, and whether recovery is possible.

10. Add encrypted vault migration/versioning infrastructure.
   - Goal: future file-format and metadata evolution should be deliberate and reversible.

11. Expand SYSOP into a real operator console.
   - Goal: give power users better audits, previews, stale-draft scans, orphan scans, and repair guidance.

12. Make exports feel finished.
   - Goal: better filenames, richer metadata controls, and safer review before plaintext leaves the vault.

13. Add real review dashboards for writing cadence and themes.
   - Goal: weekly/monthly reflection should be a first-class use case, not just CLI output.

14. Improve soundtrack behavior into a reliable optional feature.
   - Goal: clear controls, good fallbacks, obvious status, and no launch-time weirdness.

15. Add productized backup retention setup and visibility.
   - Goal: users should understand daily/weekly/monthly retention without reading raw settings.

16. Improve menu discoverability for rarely used power features.
   - Goal: AI tools, SYSOP tools, macros, and recovery paths should be findable without polluting the core journal flow.

17. Prepare the `v3.0` foundation work.
   - Goal: isolate app/service logic so web mode and eventual Windows support are realistic follow-ons.

18. Add an in-app `What changed in this release` surface.
   - Goal: keep new features discoverable without forcing users to read GitHub release notes.

19. Tighten the first save and next-entry confirmation flow even further.
   - Goal: after the first successful save, users should clearly understand same-day next entry vs next blank day.

20. Run and record a second-environment human certification pass for every stable release.
   - Goal: stable tags should always have at least one clean-account or second-machine proof point recorded.

## 10 minor steps

1. Keep installer menu wording aligned with actual outputs.
2. Keep a short `Release QA Notes` section in release notes whenever validation hits environment-only failures.
3. Trim overly long installer guide excerpts so they feel like guidance, not a doc dump.
4. Keep the free-disk note visible in release/distribution docs for universal packaging runs.
5. Normalize all references to `BlueScreen Journal` vs legacy `Personal Journal` language.
6. Tighten command/help examples so the most common flows appear first.
7. Add one-line descriptions to more menu items in picker overlays.
8. Audit all top-level docs for stale version references after each release bump.
9. Keep the footer/status language plain and non-jargony as new features land.
10. Verify the nostalgia snapshot artifact is reviewed on every PR and release build.
