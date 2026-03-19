# Changelog

## Unreleased

## v1.2.1

- upgraded CI/release workflow action versions for current GitHub runner compatibility
- replaced workflow release publication action with `gh release` CLI publishing
- added `scripts/manual-smoke-gui-terminals.sh` for Terminal.app and iTerm2 installer/launch smoke checks
- added scoped `v1.3.0` milestone plan in `docs/V1_3_PLAN.md`
- updated stable docs/install pointers and release notes to `v1.2.1`

## v1.2.0

- added `HELP -> First-Run Tour` so onboarding is reachable from menus at any time
- added `TOOLS -> Journal Health` with lock/save/integrity/backup/conflict/sync visibility
- added `scripts/stability-gate.sh` and integrated it into QA + release automation
- updated stable docs/install references and release notes to `v1.2.0`

## v1.1.2

- polished installer completion output with a clearer first-run flow and menu-first guidance
- expanded the installer post-install menu with setup, keyboard cheat sheet, and doctor + PATH repair actions
- improved in-app onboarding hints for menu discovery, unlock state guidance, and quick-save discoverability
- expanded UX regression coverage with new functional tests for menu/help roundtrips and unlock/menu hint behavior
- hardened release smoke tests to assert first-run installer messaging so docs/install copy regressions are caught earlier

## v1.1.1

- fixed installer post-install launch path to open the TUI with a PTY wrapper, preventing `Failed to initialize input reader`
- fixed bootstrap install/update flow to run the current top-level installer logic for release archives
- removed manual PATH export prompts from installer completion copy and clarified next-step messaging
- refreshed generated product screenshots and docs release surfaces for the current compact/menu-first UI

## v1.1.0

- added search power controls: `--range`, `--match-mode all|any|phrase`, `--hits-per-entry`, `--sort newest|oldest|relevance`, and `--summary`
- added timeline preset management: `--save-preset`, `--preset`, `--list-presets`, and `--delete-preset`
- added timeline calendar grouping output with `--group-by day|week|month`
- added review `--range` shortcuts and per-run word-goal analytics with `--goal`
- expanded automated coverage with new unit tests for range presets, grouping rollups, search summary/ranking, timeline preset rendering, and word-goal stats
- added optional AI reflection surfaces: `bsj ai summary`, `bsj ai coach`, plus `TOOLS -> AI Summary (Optional)` and `TOOLS -> AI Coach Mode (Optional)` in the TUI
- added local heuristic fallback for AI summary/coaching when remote AI is not configured, preserving default local-first behavior

## v1.0.3

- added advanced review and timeline CLI capabilities: range-bounded review, JSON review output, minimum top-count thresholds, weekday/mood/metadata-presence timeline filters, and timeline summary/CSV/JSON output formats
- added JSON output support for prompt listing and deterministic prompt picking for automation workflows
- expanded automated coverage for new CLI parsing and analytics/filtering behavior, including CSV escaping and timeline summary tests
- revalidated end-to-end release quality with full QA gate, packaging, artifact audit, and installer smoke tests

## v1.0.2

- fixed macOS soundtrack failures for unsupported QuickTime MIDI files by removing QuickTime as the MIDI backend
- added internal MIDI rendering fallback (MIDI -> WAV) and playback through `afplay` to avoid QuickTime compatibility popups
- added regression tests for quick-save command matching, MIDI event extraction, and WAV encoding

## v1.0.1

- added quick-save flow for rapid same-day journaling: type `**save**` on its own line and press `Enter` to save and continue into the next entry block
- added `FILE -> Quick Save + Next` menu action for the same workflow
- added same-day entry scaffold markers (`[ENTRY nn hh:mm]`) so multiple entries per day remain readable in one day view
- updated footer/help/first-run UI hints to make the quick-save workflow discoverable
- expanded TUI tests to cover quick-save command behavior and menu visibility

## v1.0.0

- declared the first stable `1.0.0` release across packaging, docs, and installer flows
- fixed soundtrack playback for MIDI sources by routing `.mid` files through a macOS QuickTime backend instead of `afplay`
- added MIDI detection coverage (extension + file header) to prevent silent soundtrack failures
- refreshed docs hub output to include quick links used by the in-app help tests

## v0.1.17

- added an in-app updater action from `TOOLS -> Check for Updates` that can launch the installer in the background
- added `HELP -> About BlueScreen Journal` plus clearer `TOOLS` soundtrack controls (`Soundtrack Source` and `Toggle Soundtrack`)
- enabled soundtrack autoplay on launch when a soundtrack source is configured (default source remains the Doogie-style MIDI URL)
- fixed installer update behavior so reruns on already-installed systems default to latest `main` source update flow
- added installer output verification for installed version and active `PATH` resolution to reduce stale-binary confusion

## v0.1.16

- improved installer completion messaging to be menu-first and first-run friendly
- clarified PATH guidance in installer output with clearer "open a new terminal or run now" wording
- refreshed first-run in-app onboarding copy to prioritize simple menu-driven actions over command-style instructions
- cleaned up top-level docs (`README`, `START_HERE`, `QUICKSTART`, `FAQ`) to make CLI diagnostics explicitly optional

## v0.1.15

- fixed bundled installer PATH setup so installs now persist `bsj` into shell startup files automatically
- added shell-specific PATH persistence coverage for zsh (`.zprofile` and `.zshrc`) and bash (`.bash_profile` and `.bashrc`)
- expanded installer smoke tests to validate PATH persistence in isolated homes and to cover both direct and bootstrap install paths
- fixed smoke archive auto-selection to use the newest versioned archive instead of stale `latest` aliases

## v0.1.14

- fixed installer `set -u` failures when no install override arguments are forwarded to the bundled installer
- hardened bootstrap delegation for both prebuilt and source install modes to handle empty argument lists safely
- added a smoke-test regression path that exercises bootstrap install with no forwarded install arguments

## v0.1.13

- polished the global search overlay so live typing no longer steals focus into results
- added quick search navigation shortcuts (`Ctrl+N`, `Ctrl+P`) plus fast clear/recall (`Ctrl+L`, `Ctrl+R`)
- added new date scope presets for last 7 days and year-to-date
- persisted search range filters between overlay opens
- retained selected matches across reruns when the same result remains in the result set
- improved search menu and overlay guidance to make range and navigation controls easier to discover

## v0.1.12

- optimized picker overlays by caching lowercase filter haystacks and filtered index lists instead of rebuilding them on every redraw and navigation event
- optimized live document stats so footer and goal/status reporting reuse cached counts instead of reconstructing full entry text repeatedly
- added direct zero-allocation buffer stats for lines, words, and characters to reduce hot-path editor overhead
- refreshed the TUI tests around direct buffer replacement so stats stay correct after edits, loads, and recovery flows

## v0.1.11

- fixed encrypted backup filename collisions that could overwrite a snapshot when two backups were created within the same second
- made backup timestamp parsing backward-compatible with both legacy second-only names and new fractional-second names
- added regression tests for rapid back-to-back backup creation and mixed backup timestamp parsing

## v0.1.10

- made export history actionable by reopening a prefilled export prompt instead of a dead-end info screen
- made backup history actionable by jumping straight into restore with the chosen encrypted backup selected
- turned Sync Center into an actionable control surface with run, snapshot, history, diagnostics, settings, and integrity actions
- added focused TUI tests for the new export-history, backup-history, and sync-center workflows
- revalidated the release bundle and installer smoke path after the remaining feature pass

## v0.1.9

- fixed overlay sizing bugs that clipped help, conflict, date, index, sync, and settings screens
- improved setup, unlock, export, restore, and conflict copy so first-run and recovery flows are clearer
- kept a compact `Ctrl+K` commands hint in the footer when the full function-key strip is hidden
- added render-smoke tests that draw major screens and overlays at real terminal sizes
- revalidated the product with format, clippy, tests, packaging, and installer smoke checks

## v0.1.8

- added a large writing-tools pass with line duplication, deletion, line movement, blank-line insertion, divider insertion, and date/time/stat/metadata stamps
- added quick export actions, export history, and a backup policy surface inside the TUI
- added favorite-to-favorite jumping, random saved-entry jumping, richer index filters, and saved-day/month jumps in the calendar
- added search scope presets for today, this month, and all time, plus encrypted cache status reporting
- added new display/settings controls for 12-hour clock mode, seconds, ruler visibility, and footer legend visibility
- expanded review, dashboard, help, and quickstart surfaces so more of the product is self-describing in the app itself

## v0.1.7

- added a command palette so major in-product actions are reachable from one searchable overlay
- added recent entries and favorite dates pickers for faster time navigation
- added recent search query recall and reusable writing prompts
- added a daily word goal setting plus live goal progress in the UI
- added session timing and richer dashboard telemetry
- added persistent sync history so past sync outcomes are visible inside the product
- expanded settings and product reporting to expose the new state cleanly

## v0.1.6

- added a DOS-style writing ruler with live cursor position emphasis
- added live document stats in the footer
- added a status dashboard and backup cleanup preview inside the TUI
- added typed date jump in the calendar overlay
- added live filter and sort controls for the real index view
- made global search rerun live as query and date filters change
- expanded selected-result detail in the search overlay

## v0.1.5

- added troubleshooting, sync, backup/restore, terminal, privacy, and macro operator guides
- added roadmap and contributing docs plus a pull request template
- added new built-in guide topics for troubleshooting, sync, backup, macros, terminal, and privacy
- improved packaged-install validation so the new operator docs ship in release bundles

## v0.1.4

- added docs hub, quickstart, FAQ, comparison guide, support policy, and security policy
- added GitHub bug report and feature request templates
- added new built-in guide topics for quickstart, FAQ, and support
- improved package/install behavior so docs and nested assets ship together

## v0.1.3

- added README hero GIF and screenshot gallery
- added release notes file and polished GitHub release body
- preserved doc assets in packaged installs and release bundles

## v0.1.2

- product-facing docs pass
- product guide and datasheet
- release packaging/docs polish

## v0.1.1

- universal macOS releases and privacy-audit automation
- GitHub Actions CI and release workflows

## v0.1.0

- initial public packaging and release line
