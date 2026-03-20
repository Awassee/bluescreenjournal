# v2.1.2 Patch Plan

This patch line follows the `v2.1.1` installer/status release.

The goal is to keep tightening trust and first-run usability without widening the product too aggressively.

## Candidate patch work

1. Add a concise first-run key cheat sheet guide.
   - Acceptance:
   - installer/help/menu surfaces can point at a short key guide
   - users can learn `save`, `menus`, `calendar`, `index`, and `search` in under a minute

2. Add a second-environment public install check.
   - Acceptance:
   - one install is run on a second Mac or clean user account
   - results are recorded in release notes or this plan

3. Surface installer asset selection more clearly in docs and diagnostics.
   - Acceptance:
   - docs explain how the installer chooses published assets
   - release runbook includes tag/main installer checks

4. Add release QA notes for environment-only failures.
   - Acceptance:
   - release notes can record disk pressure, CDN lag, or shell-environment issues without implying product regressions

5. Trim overly long installer guide excerpts.
   - Acceptance:
   - installer menu outputs feel like quick guidance, not pasted manuals

6. Keep top-level docs aligned with the current stable release and next patch scope.
   - Acceptance:
   - README, docs index, roadmap, and release notes all point at the current release line and next patch plan
