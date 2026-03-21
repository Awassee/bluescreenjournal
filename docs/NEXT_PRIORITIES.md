# Next Priorities

This is the current prioritized backlog after `v2.2.3`.

## Recently shipped in v2.2.3

- fixed the same-terminal installer handoff for the real `curl | bash` plus menu option `1` launch path
- added a PTY-backed TUI handoff so first-run launch no longer dies with `Failed to initialize input reader`
- expanded installer smoke coverage to verify real TUI startup after a piped installer run
- pushed the stable line forward to `v2.2.3`

## Current major steps

1. Improve conflict merge ergonomics.
   - Goal: make competing heads understandable and resolvable without fear.

2. Add importers for plaintext journal formats into encrypted vault revisions.
   - Goal: let new users migrate from old diaries, Markdown journals, and text files.

3. Strengthen search as a review tool.
   - Goal: saved searches, recurring prompts, and review workflows should feel instant and intentional.

4. Deepen spellcheck into a real writing assistant.
   - Goal: personal dictionary, session dictionary UX, review-before-save signals, and better typo explanations.

5. Add safer restore and recovery drills in-product.
   - Goal: backup and restore should feel tested, not theoretical.

6. Improve cloud sync observability.
   - Goal: users should know which backend is active, what remote is selected, and whether recovery is possible.

7. Add encrypted vault migration/versioning infrastructure.
   - Goal: future file-format and metadata evolution should be deliberate and reversible.

8. Expand SYSOP into a real operator console.
   - Goal: give power users better audits, previews, stale-draft scans, orphan scans, and repair guidance.

9. Make exports feel finished.
   - Goal: better filenames, richer metadata controls, and safer review before plaintext leaves the vault.

10. Add real review dashboards for writing cadence and themes.
   - Goal: weekly/monthly reflection should be a first-class use case, not just CLI output.

11. Improve soundtrack behavior into a reliable optional feature.
   - Goal: clear controls, good fallbacks, obvious status, and no launch-time weirdness.

12. Add productized backup retention setup and visibility.
   - Goal: users should understand daily/weekly/monthly retention without reading raw settings.

13. Improve menu discoverability for rarely used power features.
   - Goal: AI tools, SYSOP tools, macros, and recovery paths should be findable without polluting the core journal flow.

14. Prepare the `v3.0` foundation work.
   - Goal: isolate app/service logic so web mode and eventual Windows support are realistic follow-ons.

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
