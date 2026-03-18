# Changelog

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
