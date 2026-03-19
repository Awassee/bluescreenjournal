# v1.3 UX Checklist

Date: 2026-03-19

## Daily Write Flow

- [x] Launch into editor with date and entry context visible.
- [x] Type immediately without setup friction when already unlocked.
- [x] Save state is obvious (`UNSAVED`, `REVISION SAVED`, `DRAFT AUTOSAVED`).
- [x] Quick save (`**save**` + Enter) clears to a new same-day page.
- [x] Save receipt is available in `FILE -> Save Receipt`.

## Archive Flow

- [x] Calendar and Index remain primary archive navigation surfaces.
- [x] Deep backward day-jumps require repeat confirmation.
- [x] Saved-entry jump remains fast for intentional timeline browsing.

## Discoverability

- [x] All core flows are reachable from top menus.
- [x] Help card lists function keys plus spellcheck command.
- [x] Daily flow guidance is available via `HELP -> Daily Flow Coach`.
- [x] Spellcheck is discoverable in `EDIT` and via `Ctrl+Shift+F`.

## Terminal Robustness

- [x] Resize event remains non-panicking.
- [x] Small-terminal warning stays actionable.
- [x] Menu/open/close/edit transition tests pass without stale glyph artifacts.

## Test Mapping

- `tui::app::tests::human_like_batch_entry_flow_saves_and_reloads_many_days`
- `tui::app::tests::functional_menu_to_overlay_back_to_editor_preserves_typing_flow`
- `tui::app::tests::ctrl_shift_f_opens_spellcheck_picker`
- `tui::app::tests::spellcheck_picker_apply_replaces_misspelling`
- `tui::app::tests::spellcheck_autofix_common_typos_updates_buffer`
- `tui::app::tests::save_receipt_overlay_shows_after_manual_save`
- `tui::app::tests::archive_guard_requires_repeat_for_old_backward_jump`
