#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source "$ROOT_DIR/scripts/installer-test-lib.sh"
ARCHIVE=""

usage() {
  cat <<'EOF'
installer-action-matrix.sh

Runs a focused matrix of installer action modes and destructive flows in temp directories.

Usage:
  ./scripts/installer-action-matrix.sh [--archive PATH]

Without --archive, the latest versioned archive under dist/ is used, building one first if needed.
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --help|-h)
      usage
      exit 0
      ;;
    --archive)
      [[ $# -ge 2 ]] || { echo "--archive requires a value" >&2; exit 1; }
      ARCHIVE="$2"
      shift 2
      ;;
    *)
      echo "Unknown option: $1" >&2
      exit 1
      ;;
  esac
done

if [[ -z "$ARCHIVE" ]]; then
  "$ROOT_DIR/scripts/package-release.sh"
  versioned_archives=()
  while IFS= read -r -d '' archive_path; do
    versioned_archives+=("$archive_path")
  done < <(find "$ROOT_DIR/dist" -maxdepth 1 -type f -name 'bsj-[0-9]*-*.tar.gz' -print0)
  [[ "${#versioned_archives[@]}" -gt 0 ]] || { echo "No versioned archives found under $ROOT_DIR/dist" >&2; exit 1; }
  ARCHIVE="$(ls -t "${versioned_archives[@]}" | head -n 1)"
fi

[[ -f "$ARCHIVE" ]] || { echo "Archive not found: $ARCHIVE" >&2; exit 1; }

TMP_ROOT="${TMPDIR:-/tmp}"
mkdir -p "$TMP_ROOT"
TMP_DIR="$(mktemp -d "$TMP_ROOT/bsj-installer-actions.XXXXXX")"
SMOKE_PATH="/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin:${PATH:-}"

ABOUT_HOME="$TMP_DIR/about-home"
DOCTOR_HOME="$TMP_DIR/doctor-home"
REPAIR_HOME="$TMP_DIR/repair-home"
UNINSTALL_HOME="$TMP_DIR/uninstall-home"
RESET_HOME="$TMP_DIR/reset-home"

REPAIR_PREFIX="$TMP_DIR/repair-prefix"
UNINSTALL_PREFIX="$TMP_DIR/uninstall-prefix"
RESET_PREFIX="$TMP_DIR/reset-prefix"

ABOUT_LOG="$TMP_DIR/about.log"
DOCTOR_LOG="$TMP_DIR/doctor.log"
REPAIR_INSTALL_LOG="$TMP_DIR/repair-install.log"
REPAIR_LOG_1="$TMP_DIR/repair-path-1.log"
REPAIR_LOG_2="$TMP_DIR/repair-path-2.log"
UNINSTALL_INSTALL_LOG="$TMP_DIR/uninstall-install.log"
UNINSTALL_LOG="$TMP_DIR/uninstall.log"
RESET_INSTALL_LOG="$TMP_DIR/reset-install.log"
RESET_LOG="$TMP_DIR/factory-reset.log"

mkdir -p "$ABOUT_HOME" "$DOCTOR_HOME" "$REPAIR_HOME" "$UNINSTALL_HOME" "$RESET_HOME"

run_with_timeout 30 "installer about" \
  env HOME="$ABOUT_HOME" SHELL=/bin/zsh "$ROOT_DIR/install.sh" --about \
  >"$ABOUT_LOG" 2>&1
assert_log_contains "$ABOUT_LOG" "BlueScreen Journal Installer"
assert_log_contains "$ABOUT_LOG" "install/update stable prebuilt bundles"
assert_log_contains "$ABOUT_LOG" "uninstall or factory reset"

run_with_timeout 120 "installer doctor" \
  env PATH="$SMOKE_PATH" HOME="$DOCTOR_HOME" SHELL=/bin/zsh "$ROOT_DIR/install.sh" --doctor \
  >"$DOCTOR_LOG" 2>&1
assert_log_contains "$DOCTOR_LOG" "Installer Doctor"
assert_log_contains "$DOCTOR_LOG" "Doctor summary:"
assert_log_contains "$DOCTOR_LOG" "GitHub API reachability"

run_with_timeout 180 "prebuilt install for repair-path checks" \
  env HOME="$REPAIR_HOME" SHELL=/bin/zsh "$ROOT_DIR/install.sh" --prebuilt --archive "$ARCHIVE" --prefix "$REPAIR_PREFIX" \
  >"$REPAIR_INSTALL_LOG" 2>&1
"$REPAIR_PREFIX/bin/bsj" --help >/dev/null

run_with_timeout 30 "repair-path run 1" \
  env PATH="$SMOKE_PATH" HOME="$REPAIR_HOME" SHELL=/bin/zsh "$ROOT_DIR/install.sh" --repair-path --prefix "$REPAIR_PREFIX" \
  >"$REPAIR_LOG_1" 2>&1
run_with_timeout 30 "repair-path run 2" \
  env PATH="$SMOKE_PATH" HOME="$REPAIR_HOME" SHELL=/bin/zsh "$ROOT_DIR/install.sh" --repair-path --prefix "$REPAIR_PREFIX" \
  >"$REPAIR_LOG_2" 2>&1
assert_log_contains "$REPAIR_LOG_2" "PATH update already present in shell config."
assert_path_entry_count "$REPAIR_HOME/.zprofile" "$REPAIR_PREFIX/bin" 1
assert_path_entry_count "$REPAIR_HOME/.zshrc" "$REPAIR_PREFIX/bin" 1
assert_path_entry_count "$REPAIR_HOME/.bash_profile" "$REPAIR_PREFIX/bin" 1
assert_path_entry_count "$REPAIR_HOME/.bashrc" "$REPAIR_PREFIX/bin" 1
assert_path_entry_count "$REPAIR_HOME/.config/fish/config.fish" "$REPAIR_PREFIX/bin" 1

run_with_timeout 180 "prebuilt install for uninstall checks" \
  env HOME="$UNINSTALL_HOME" SHELL=/bin/zsh "$ROOT_DIR/install.sh" --prebuilt --archive "$ARCHIVE" --prefix "$UNINSTALL_PREFIX" \
  >"$UNINSTALL_INSTALL_LOG" 2>&1
mkdir -p "$UNINSTALL_HOME/Documents/BlueScreenJournal"
printf '{\"version\":1}\n' > "$UNINSTALL_HOME/Documents/BlueScreenJournal/vault.json"

run_with_timeout 60 "uninstall keeps data" \
  env PATH="$UNINSTALL_PREFIX/bin:$SMOKE_PATH" HOME="$UNINSTALL_HOME" SHELL=/bin/zsh "$ROOT_DIR/install.sh" --uninstall --prefix "$UNINSTALL_PREFIX" --yes \
  >"$UNINSTALL_LOG" 2>&1
assert_log_contains "$UNINSTALL_LOG" "Uninstall complete."
assert_log_contains "$UNINSTALL_LOG" "Data was preserved."
[[ ! -e "$UNINSTALL_PREFIX/bin/bsj" ]] || { echo "Uninstall should remove the binary" >&2; exit 1; }
[[ -f "$UNINSTALL_HOME/Documents/BlueScreenJournal/vault.json" ]] || { echo "Uninstall should preserve journal data" >&2; exit 1; }

run_with_timeout 180 "prebuilt install for factory reset checks" \
  env HOME="$RESET_HOME" SHELL=/bin/zsh "$ROOT_DIR/install.sh" --prebuilt --archive "$ARCHIVE" --prefix "$RESET_PREFIX" \
  >"$RESET_INSTALL_LOG" 2>&1
mkdir -p \
  "$RESET_HOME/Documents/BlueScreenJournal" \
  "$RESET_HOME/Documents/BlueScreenJournal-Sync" \
  "$RESET_HOME/Library/Application Support/bsj" \
  "$RESET_HOME/Library/Logs/bsj"
printf '{\"vault_path\":\"~/Documents/BlueScreenJournal\",\"sync_target_path\":\"~/Documents/BlueScreenJournal-Sync\"}\n' \
  > "$RESET_HOME/Library/Application Support/bsj/config.json"
printf '{\"version\":1}\n' > "$RESET_HOME/Documents/BlueScreenJournal/vault.json"
printf 'encrypted-blob\n' > "$RESET_HOME/Documents/BlueScreenJournal-Sync/rev-000001.bsj.enc"
printf 'log line\n' > "$RESET_HOME/Library/Logs/bsj/bsj.log"

run_with_timeout 60 "factory reset removes install and local data" \
  env PATH="$RESET_PREFIX/bin:$SMOKE_PATH" HOME="$RESET_HOME" SHELL=/bin/zsh "$ROOT_DIR/install.sh" --factory-reset --prefix "$RESET_PREFIX" --yes \
  >"$RESET_LOG" 2>&1
assert_log_contains "$RESET_LOG" "Factory reset complete."
[[ ! -e "$RESET_PREFIX/bin/bsj" ]] || { echo "Factory reset should remove the binary" >&2; exit 1; }
[[ ! -e "$RESET_HOME/Documents/BlueScreenJournal" ]] || { echo "Factory reset should remove the vault path" >&2; exit 1; }
[[ ! -e "$RESET_HOME/Documents/BlueScreenJournal-Sync" ]] || { echo "Factory reset should remove the sync path" >&2; exit 1; }
[[ ! -e "$RESET_HOME/Library/Application Support/bsj" ]] || { echo "Factory reset should remove config" >&2; exit 1; }
[[ ! -e "$RESET_HOME/Library/Logs/bsj" ]] || { echo "Factory reset should remove logs" >&2; exit 1; }

cat <<EOF
Installer action matrix passed:
  Archive: $ARCHIVE
  Temp dir: $TMP_DIR
EOF
