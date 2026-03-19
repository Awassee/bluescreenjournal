# BlueScreen Journal: Next 10 Major Features (Implemented)

This pass focused on high-leverage search and timeline capabilities that improve retrieval speed, precision, and operator workflows without relaxing the app's local-first security model.

## 1) JSON Search Output
- **Problem:** CLI search was human-only output, hard to automate.
- **Spec:** `bsj search ... --json` prints structured match objects.
- **Shipped:** Yes.
- **Test coverage:** `tests::cli_parses_advanced_search_flags`

## 2) Search Result Limits
- **Problem:** Large vault queries can produce too much output.
- **Spec:** `bsj search ... --limit N` truncates to first `N` matches.
- **Shipped:** Yes.
- **Test coverage:** parser coverage in `tests::cli_parses_advanced_search_flags`

## 3) Count-Only Search Mode
- **Problem:** Sometimes only match count matters for audits/reports.
- **Spec:** `bsj search ... --count-only` prints count (or JSON count with `--json`).
- **Shipped:** Yes.
- **Test coverage:** parser coverage in `tests::cli_parses_advanced_search_flags`

## 4) Case-Sensitive Search
- **Problem:** Proper nouns and acronyms need exact case matching.
- **Spec:** `bsj search ... --case-sensitive`.
- **Shipped:** Yes.
- **Test coverage:** `search::tests::case_sensitive_search_only_matches_exact_case`

## 5) Whole-Word Search
- **Problem:** Partial substring matches cause noise.
- **Spec:** `bsj search ... --whole-word` enforces boundary matching.
- **Shipped:** Yes.
- **Test coverage:** `search::tests::whole_word_search_ignores_partial_matches`

## 6) Configurable Snippet Context
- **Problem:** Context needs vary by workflow.
- **Spec:** `bsj search ... --context N` controls snippet context width.
- **Shipped:** Yes.
- **Test coverage:** `search::tests::snippet_context_size_follows_options`

## 7) Timeline Text Query Filter
- **Problem:** Timeline scanning needed quick semantic narrowing.
- **Spec:** `bsj timeline --query <text>` filters by preview + metadata.
- **Shipped:** Yes.
- **Test coverage:** `tests::timeline_query_filter_matches_preview_and_metadata`

## 8) Timeline Tag Filter
- **Problem:** Tag-driven reviews were manual.
- **Spec:** `bsj timeline --tag <tag>` (case-insensitive exact tag match).
- **Shipped:** Yes.
- **Test coverage:** `tests::timeline_tag_person_and_project_filters_are_case_insensitive`

## 9) Timeline Person Filter
- **Problem:** Relationship-focused retrospectives were slower than needed.
- **Spec:** `bsj timeline --person <name>` (case-insensitive exact person match).
- **Shipped:** Yes.
- **Test coverage:** `tests::timeline_tag_person_and_project_filters_are_case_insensitive`

## 10) Timeline Project Filter
- **Problem:** Project-specific review workflow required manual sorting.
- **Spec:** `bsj timeline --project <project>` (case-insensitive exact project match).
- **Shipped:** Yes.
- **Test coverage:** `tests::timeline_tag_person_and_project_filters_are_case_insensitive`

## Validation
- `cargo fmt`
- `cargo clippy --all-targets -- -D warnings`
- `cargo test -q`
- `./scripts/qa-gate.sh`
