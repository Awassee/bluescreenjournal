# v1.3.2 Plan

Status: completed on `main`, pending public release cut.

## Scope

1. Sync parity across config, doctor, CLI, and TUI
2. Menu-driven direct cloud connector setup
3. Installer and updater hardening
4. Current-release docs and release-note consistency
5. Trust/status polish for sync and recovery surfaces

## Delivered

- Added config-backed sync defaults:
  - `sync_backend_preference`
  - `gdrive_folder_id`
  - `dropbox_root`
- Made CLI and TUI resolve sync backend in this order:
  - explicit args
  - `BSJ_SYNC_BACKEND` env override
  - saved Setup default
  - folder auto fallback
- Extended `doctor`, Settings Summary, Dashboard, Journal Health, Sync Center, and Cloud Status to show the effective sync mode more clearly.
- Expanded `SETUP -> Cloud Provider Setup` into a cloud sync control surface that now covers:
  - folder-based provider setup
  - direct Google Drive API mode
  - direct Dropbox API mode
  - editable backend default
- Hardened installer launch behavior with a direct-launch fallback if PTY launch fails.
- Improved installer completion copy and smoke coverage for the new menu-first cloud setup flow.

## Acceptance criteria

- `doctor` recognizes `gdrive` and `dropbox` setups instead of treating them as unknown backends.
- TUI sync/status/recovery flows can use saved backend defaults without requiring `BSJ_SYNC_BACKEND`.
- Menu users can select direct Google Drive or Dropbox mode without dropping immediately into CLI-only instructions.
- Installer output points new users to in-product menus instead of making post-install next steps feel shell-centric.
- Docs consistently describe folder sync, direct API sync, env-backed secrets, and Setup menu flows.
