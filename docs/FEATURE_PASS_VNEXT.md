# BlueScreen Journal: v1.0.3 Feature Pass

This pass focused on analytics, automation output, and public docs/GitHub surface consistency.

## 1) Range-bounded review metrics
- **Spec:** `bsj review --from YYYY-MM-DD --to YYYY-MM-DD`
- **Value:** weekly/monthly check-ins without manual date filtering
- **Coverage:** `tests::cli_parses_review_range_and_json_flags`

## 2) JSON review output
- **Spec:** `bsj review --json`
- **Value:** script and dashboard friendly reporting
- **Coverage:** parser + summary unit tests

## 3) Review top-result thresholding
- **Spec:** `bsj review --min-count N`
- **Value:** removes noisy low-frequency metadata from top lists
- **Coverage:** `tests::review_summary_applies_range_and_min_count`

## 4) Timeline format selection
- **Spec:** `bsj timeline --format text|json|csv`
- **Value:** one command surface for human, automation, and spreadsheet use
- **Coverage:** `tests::cli_parses_timeline_format_and_extended_filters`

## 5) Timeline aggregate mode
- **Spec:** `bsj timeline --summary`
- **Value:** fast at-a-glance journal health metrics
- **Coverage:** `tests::timeline_summary_counts_conflicts_favorites_and_moods`

## 6) Timeline mood filter
- **Spec:** `bsj timeline --mood 0..9`
- **Value:** mood-specific retrospectives and trend analysis
- **Coverage:** `tests::timeline_filters_mood_presence_and_weekday`

## 7) Timeline metadata-presence filters
- **Spec:** `--has-tags --has-people --has-project`
- **Value:** quickly isolate richly-annotated entries
- **Coverage:** `tests::timeline_filters_mood_presence_and_weekday`

## 8) Timeline weekday filter
- **Spec:** `bsj timeline --weekday mon,tue,...`
- **Value:** weekday behavior analysis across routines
- **Coverage:** `tests::timeline_filters_mood_presence_and_weekday`

## 9) Prompt library JSON output
- **Spec:** `bsj prompts list --json`
- **Value:** tooling-friendly prompt catalogs
- **Coverage:** `tests::cli_parses_prompts_json_flags`

## 10) Prompt pick JSON output
- **Spec:** `bsj prompts pick --json`
- **Value:** deterministic prompt generation for automations
- **Coverage:** `tests::cli_parses_prompts_json_flags`

## Documentation and GitHub surfaces updated

- README command/reference updates
- setup/quickstart/troubleshooting/settings/docs hub updates
- issue-template and PR-template updates
- release notes added for `v1.0.3`

## Validation

- `cargo fmt --all`
- `cargo clippy --all-targets -- -D warnings`
- `cargo test --all-targets`
- `./scripts/qa-gate.sh`
