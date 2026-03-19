#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

iterations="${BSJ_STABILITY_ITERATIONS:-4}"
if ! [[ "$iterations" =~ ^[0-9]+$ ]] || [[ "$iterations" -lt 1 ]]; then
  echo "BSJ_STABILITY_ITERATIONS must be a positive integer (got: $iterations)" >&2
  exit 1
fi

CRITICAL_TESTS=(
  "tui::app::tests::menu_action_roundtrip_returns_to_clean_editor_frame"
  "tui::app::tests::shortened_line_repaint_does_not_leave_tail_ghost_text"
  "tui::app::tests::typing_wraps_when_line_exceeds_viewport_width"
  "tui::app::tests::keybinding_function_keys_route_to_expected_actions"
)

echo "==> Stability gate: ${iterations} pass(es) over critical UX tests"
for pass in $(seq 1 "$iterations"); do
  echo "==> Stability pass $pass/$iterations"
  for test_name in "${CRITICAL_TESTS[@]}"; do
    echo "   -> $test_name"
    cargo test --all-targets "$test_name"
  done
done

echo "==> Stability gate passed"
