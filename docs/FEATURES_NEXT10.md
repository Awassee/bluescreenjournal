# BlueScreen Journal - Next 10 Feature Pack

This pass defines and ships ten workflow features in order, keeping the nostalgic menu-first UX while preserving encryption and local-first behavior.

## 1) Save + Next Day
- Problem: ending a day currently requires multiple actions.
- Spec: add a single action that saves the current entry and opens the next new day.
- Surface: `FILE -> Save + Next Day` and `Ctrl+Shift+S`.
- Done when: action saves revision (if dirty) and opens a fresh next-day page.

## 2) Save + Lock
- Problem: users often want to secure the vault immediately after saving.
- Spec: add a one-step save-then-lock action.
- Surface: `FILE -> Save + Lock` and `Ctrl+Shift+L`.
- Done when: action saves revision (if dirty), wipes unlocked state, and returns to unlock prompt.

## 3) Yesterday Jump
- Problem: day-by-day navigation lacks a mnemonic “yesterday” shortcut.
- Spec: add explicit yesterday jump in navigation menu and hotkey.
- Surface: `GO -> Yesterday`, `Alt+-`.
- Done when: selected date moves back one day and status updates.

## 4) Tomorrow Jump
- Problem: users want a matching forward shortcut.
- Spec: add explicit tomorrow jump in navigation menu and hotkey.
- Surface: `GO -> Tomorrow`, `Alt+=`.
- Done when: selected date moves forward one day and status updates.

## 5) Extract Hashtags to Metadata
- Problem: users type `#tags` in body text but metadata may remain stale.
- Spec: parse body hashtags and merge them into entry metadata tags.
- Surface: `EDIT -> Extract #Tags to Metadata`.
- Done when: unique normalized hashtags are merged, without duplicates.

## 6) Mood Up
- Problem: mood entry should be fast and keyboard-first.
- Spec: add one-step mood increment action, clamped to `0..9`.
- Surface: `EDIT -> Mood Up`.
- Done when: mood increases or clamps at `9`, status reflects value.

## 7) Mood Down
- Problem: mood adjustment also needs decrement flow.
- Spec: add one-step mood decrement action, clamped to `0..9`.
- Surface: `EDIT -> Mood Down`.
- Done when: mood decreases or clamps at `0`, status reflects value.

## 8) Insert Daily Starter
- Problem: users want a fast structured skeleton for reflective writing.
- Spec: insert a standardized starter block with entry/date context.
- Surface: `EDIT -> Insert Daily Starter`.
- Done when: template lines are inserted at cursor and editor stays consistent.

## 9) Today Brief Card
- Problem: users need an at-a-glance current-state summary.
- Spec: show a concise operational brief (save/page state, word stats, streak, pulse).
- Surface: `TOOLS -> Today Brief`.
- Done when: info overlay opens with current status and suggested next step.

## 10) Week Compass Card
- Problem: users need weekly orientation without leaving the TUI.
- Spec: show 7-day window metrics, conflict count, mood mix, and top metadata signals.
- Surface: `TOOLS -> Week Compass`.
- Done when: info overlay opens from unlocked vault and displays weekly summary lines.
