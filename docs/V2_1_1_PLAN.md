# v2.1.1 Execution Plan

This patch line is about tightening the release experience around the already-published `v2.1.0` build.

## Immediate next steps completed

1. Public installer smoke gate is now part of the release flow.
   - Status: done in `v2.1.0`
   - Source: [scripts/smoke-public-install.sh](../scripts/smoke-public-install.sh), [.github/workflows/release.yml](../.github/workflows/release.yml)

2. Human-style live install check was run against the public `v2.1.0` tag on `2026-03-20`.
   - Command shape:
     - `curl -fsSL https://raw.githubusercontent.com/Awassee/bluescreenjournal/v2.1.0/install.sh | bash -s -- --prebuilt --version v2.1.0`
   - Result: pass after clearing local disk pressure on the test machine

## Live installer findings from the human check

What worked well:

1. The tagged public installer selected the correct published asset:
   - `bsj-universal-apple-darwin.tar.gz`

2. PATH repair messaging was clear enough to get a user to a working `bsj` command.

3. The post-install menu is now a viable handoff surface:
   - guide
   - quickstart
   - doctor
   - command help

4. The installer output correctly emphasizes the menu-first product model instead of forcing CLI-only setup.

What still needs cleanup:

1. Menu option `4` said `keyboard/menu cheat sheet` but printed the quickstart excerpt.
   - Fix: relabel option `4` to `Print quickstart + key cheat sheet`
   - Status: done on `main`

2. We still need a second-machine, human-run install check outside the primary workstation.
   - This is the best way to catch environment drift, shell-profile oddities, and hardware-specific surprises.

3. The release runbook should explicitly call out that local disk pressure can create false-negative install failures during smoke checks.

## v2.1.1 patch candidates

1. Add a second-environment public install check.
   - Acceptance:
   - one install is run on a second Mac or clean user account
   - results are recorded in release notes or this plan

2. Surface installer asset selection more clearly in doctor/output docs.
   - Acceptance:
   - installer doctor shows which asset class would be selected
   - distribution docs explain tag-vs-main bootstrap expectations

3. Keep the post-install menu wording aligned with actual actions.
   - Acceptance:
   - menu labels match the exact guide/help output they trigger
   - smoke coverage checks the menu wording and selected actions

4. Add a concise real cheat-sheet surface.
   - Acceptance:
   - a short guide exists for first-run keys and menu access
   - installer option `4` can point to that exact guide instead of an excerpt workaround

5. Expand release validation notes around disk pressure and temp-space usage.
   - Acceptance:
   - release docs mention expected temp-space needs for universal packaging
   - smoke failures caused by `No space left on device` are documented as environment failures, not product regressions

6. Verify that all top-level docs point at the current patch line and roadmap.
   - Acceptance:
   - README, docs index, roadmap, and release notes all point to `v2.1.1` planning

## Not in scope for v2.1.1

1. Windows support
2. Local web mode
3. New sync providers beyond the current release line
4. Large editor workflow changes unrelated to release/install trust
