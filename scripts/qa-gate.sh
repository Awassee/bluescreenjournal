#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

echo "==> QA gate: format check"
cargo fmt --all --check

echo "==> QA gate: clippy"
cargo clippy --all-targets -- -D warnings

echo "==> QA gate: targeted UX regressions"
UX_TESTS=(
  "tui::app::tests::quick_save_line_command_saves_then_opens_clean_same_day_entry_page"
  "tui::app::tests::quick_save_menu_action_saves_and_clears_editor_to_fresh_same_day_page"
  "tui::app::tests::human_like_batch_entry_flow_saves_and_reloads_many_days"
  "tui::app::tests::mixed_input_stress_session_preserves_editor_invariants"
  "tui::app::tests::menu_action_roundtrip_returns_to_clean_editor_frame"
  "tui::app::tests::shortened_line_repaint_does_not_leave_tail_ghost_text"
  "tui::app::tests::keybinding_function_keys_route_to_expected_actions"
  "tui::app::tests::keybinding_ctrl_fallbacks_route_to_expected_actions"
  "tui::app::tests::typing_wraps_when_line_exceeds_viewport_width"
  "tui::app::tests::tab_key_inserts_five_spaces"
)

for test_name in "${UX_TESTS[@]}"; do
  echo "   -> $test_name"
  cargo test --all-targets "$test_name"
done

echo "==> QA gate: full tests"
cargo test --all-targets

echo "==> QA gate: package release bundle"
./scripts/package-release.sh

version="$(awk -F'"' '/^version = "/ { print $2; exit }' Cargo.toml)"
if [[ -z "$version" ]]; then
  echo "failed to parse package version from Cargo.toml" >&2
  exit 1
fi

archive="$(ls -1t dist/bsj-"${version}"-*.tar.gz 2>/dev/null | head -n 1)"
if [[ -z "$archive" ]]; then
  echo "failed to locate release archive for version ${version}" >&2
  exit 1
fi

echo "==> QA gate: smoke install (${archive})"
./scripts/smoke-release-install.sh --archive "$archive"

echo "==> QA gate passed"
