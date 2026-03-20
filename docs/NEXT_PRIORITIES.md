# Next Priorities

This is the current prioritized backlog after `v2.1.1`.

## 20 major steps

1. Add a real first-run cheat sheet guide and wire the installer/menu/help surfaces to it.
   - Goal: the first five minutes should feel obvious without reading long docs.

2. Build a second-machine release certification pass.
   - Goal: every stable release gets one human install/launch check outside the primary workstation.

3. Add visual artifact review to CI for key nostalgia screens.
   - Goal: regressions in layout, hints, or menu wording are visible before release.

4. Ship a trust dashboard drill-down flow.
   - Goal: from one place, users can understand vault health, verify, backup, sync, and recovery readiness.

5. Improve conflict merge ergonomics.
   - Goal: make competing heads understandable and resolvable without fear.

6. Add importers for plaintext journal formats into encrypted vault revisions.
   - Goal: let new users migrate from old diaries, Markdown journals, and text files.

7. Add a lightweight onboarding coach that disappears after first confident use.
   - Goal: teach save, next entry, next day, menus, calendar, and old-entry access without cluttering long-term use.

8. Strengthen search as a review tool.
   - Goal: saved searches, recurring prompts, and review workflows should feel instant and intentional.

9. Deepen spellcheck into a real writing assistant.
   - Goal: personal dictionary, session dictionary UX, review-before-save signals, and better typo explanations.

10. Add safer restore and recovery drills in-product.
   - Goal: backup and restore should feel tested, not theoretical.

11. Build a proper release runbook with explicit gates and failure classes.
   - Goal: distinguish product regressions from environment failures like disk pressure or CDN lag.

12. Improve cloud sync observability.
   - Goal: users should know which backend is active, what remote is selected, and whether recovery is possible.

13. Add encrypted vault migration/versioning infrastructure.
   - Goal: future file-format and metadata evolution should be deliberate and reversible.

14. Expand SYSOP into a real operator console.
   - Goal: give power users better audits, previews, stale-draft scans, orphan scans, and repair guidance.

15. Make exports feel finished.
   - Goal: better filenames, richer metadata controls, and safer review before plaintext leaves the vault.

16. Add real review dashboards for writing cadence and themes.
   - Goal: weekly/monthly reflection should be a first-class use case, not just CLI output.

17. Improve soundtrack behavior into a reliable optional feature.
   - Goal: clear controls, good fallbacks, obvious status, and no launch-time weirdness.

18. Add productized backup retention setup and visibility.
   - Goal: users should understand daily/weekly/monthly retention without reading raw settings.

19. Improve menu discoverability for rarely used power features.
   - Goal: AI tools, SYSOP tools, macros, and recovery paths should be findable without polluting the core journal flow.

20. Prepare the `v3.0` foundation work.
   - Goal: isolate app/service logic so web mode and eventual Windows support are realistic follow-ons.

## 10 minor steps

1. Keep installer menu wording aligned with actual outputs.
2. Add a short `Release QA Notes` section to release notes when validation hits environment-only failures.
3. Trim overly long installer guide excerpts so they feel like guidance, not a doc dump.
4. Add a dedicated note in docs about required free disk space for universal packaging and smoke tests.
5. Normalize all references to `BlueScreen Journal` vs legacy `Personal Journal` language.
6. Tighten command/help examples so the most common flows appear first.
7. Add one-line descriptions to more menu items in picker overlays.
8. Audit all top-level docs for stale version references after each release bump.
9. Add a tiny `What changed in this release` surface inside the app help menu.
10. Keep the footer/status language plain and non-jargony as new features land.
