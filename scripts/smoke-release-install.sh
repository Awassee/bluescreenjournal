#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source "$ROOT_DIR/scripts/installer-test-lib.sh"
ARCHIVE=""

usage() {
  cat <<'EOF'
smoke-release-install.sh

Usage:
  ./scripts/smoke-release-install.sh [--archive PATH]

Without --archive, this script builds a fresh release bundle first.
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
TMP_DIR="$(mktemp -d "$TMP_ROOT/bsj-dist-smoke.XXXXXX")"
INSTALL_PREFIX="$TMP_DIR/install-root"
SMART_PREFIX="$TMP_DIR/install-smart-root"
MENU_PREFIX="$TMP_DIR/install-menu-root"
MENU_LAUNCH_PREFIX="$TMP_DIR/install-menu-launch-root"
BOOTSTRAP_PREFIX="$TMP_DIR/bootstrap-root"
INSTALL_HOME="$TMP_DIR/install-home"
SMART_HOME="$TMP_DIR/install-home-smart"
MENU_HOME="$TMP_DIR/install-home-menu"
MENU_LAUNCH_HOME="$TMP_DIR/install-home-menu-launch"
BOOTSTRAP_HOME="$TMP_DIR/bootstrap-home"
BOOTSTRAP_NOARGS_HOME="$TMP_DIR/bootstrap-home-noargs"
BOOTSTRAP_BASH_HOME="$TMP_DIR/bootstrap-home-bash"
SOURCE_UPDATE_HOME="$TMP_DIR/source-update-home"
SOURCE_UPDATE_PREFIX="$TMP_DIR/source-update-prefix"
UPDATE_ACTION_HOME="$SOURCE_UPDATE_HOME"
UPDATE_ACTION_PREFIX="$SOURCE_UPDATE_PREFIX"
SMOKE_PATH="/usr/bin:/bin:/usr/sbin:/sbin"
INSTALL_LOG="$TMP_DIR/install-prebuilt.log"
SMART_LOG="$TMP_DIR/install-smart.log"
MENU_LOG="$TMP_DIR/install-prebuilt-menu.log"
MENU_LAUNCH_LOG="$TMP_DIR/install-prebuilt-menu-launch.log"
HANDOFF_LOG="$TMP_DIR/install-handoff.log"
SOURCE_UPDATE_LOG="$TMP_DIR/install-source-update.log"
UPDATE_ACTION_LOG="$TMP_DIR/install-update-action.log"

tar -C "$TMP_DIR" -xzf "$ARCHIVE"
BUNDLE_DIR="$(find "$TMP_DIR" -mindepth 1 -maxdepth 1 -type d | head -n 1)"
[[ -d "$BUNDLE_DIR" ]] || { echo "Bundle directory not found after extraction" >&2; exit 1; }

run_with_timeout 180 "bundled prebuilt install" \
  env HOME="$INSTALL_HOME" SHELL=/bin/zsh "$BUNDLE_DIR/install.sh" --prebuilt --prefix "$INSTALL_PREFIX" \
  >"$INSTALL_LOG" 2>&1
"$INSTALL_PREFIX/bin/bsj" --help >/dev/null
"$INSTALL_PREFIX/bin/bsj" guide setup >/dev/null
"$INSTALL_PREFIX/bin/bsj" guide distribution >/dev/null
"$INSTALL_PREFIX/bin/bsj" guide whatsnew >/dev/null
"$INSTALL_PREFIX/bin/bsj" settings >/dev/null
assert_log_contains "$INSTALL_LOG" "Quick start:"
assert_log_contains "$INSTALL_LOG" "Press F2 to save, or type **save** then Enter for quick-save + next entry"
assert_log_contains "$INSTALL_LOG" "Press Esc (or Ctrl+O) to open menus"
assert_log_contains "$INSTALL_LOG" "guide cheatsheet"
assert_log_contains "$INSTALL_LOG" "guide whatsnew"
assert_log_contains "$INSTALL_LOG" "State: copying bundled app files"
assert_log_contains "$INSTALL_LOG" "State: checking PATH integration"
assert_log_contains "$INSTALL_LOG" "State: verifying installed binary"
assert_log_contains "$INSTALL_LOG" "State: post-install summary is ready"
test -f "$INSTALL_PREFIX/share/doc/bsj/README.md"
test -f "$INSTALL_PREFIX/share/doc/bsj/LICENSE"
test -f "$INSTALL_PREFIX/share/doc/bsj/CHANGELOG.md"
test -f "$INSTALL_PREFIX/share/doc/bsj/SUPPORT.md"
test -f "$INSTALL_PREFIX/share/doc/bsj/SECURITY.md"
test -f "$INSTALL_PREFIX/share/doc/bsj/CONTRIBUTING.md"
test -f "$INSTALL_PREFIX/share/doc/bsj/ROADMAP.md"
test -f "$INSTALL_PREFIX/share/doc/bsj/docs/PRODUCT_GUIDE.md"
test -f "$INSTALL_PREFIX/share/doc/bsj/docs/TROUBLESHOOTING.md"
test -f "$INSTALL_PREFIX/share/doc/bsj/docs/SYNC_GUIDE.md"
test -f "$INSTALL_PREFIX/share/doc/bsj/docs/BACKUP_RESTORE.md"
test -f "$INSTALL_PREFIX/share/doc/bsj/docs/MACRO_GUIDE.md"
test -f "$INSTALL_PREFIX/share/doc/bsj/docs/TERMINAL_GUIDE.md"
test -f "$INSTALL_PREFIX/share/doc/bsj/docs/PRIVACY.md"
test -f "$INSTALL_PREFIX/share/doc/bsj/docs/assets/bsj-hero.gif"
test -f "$INSTALL_PREFIX/share/man/man1/bsj.1"
test -f "$INSTALL_PREFIX/share/bsj/examples/config.example.json"
test -f "$INSTALL_PREFIX/share/bash-completion/completions/bsj"
test -f "$INSTALL_PREFIX/share/zsh/site-functions/_bsj"
test -f "$INSTALL_PREFIX/share/fish/vendor_completions.d/bsj.fish"
"$ROOT_DIR/scripts/audit-release.sh" --binary "$INSTALL_PREFIX/bin/bsj"
grep -Fq "$INSTALL_PREFIX/bin" "$INSTALL_HOME/.zprofile"
grep -Fq "$INSTALL_PREFIX/bin" "$INSTALL_HOME/.zshrc"
grep -Fq "$INSTALL_PREFIX/bin" "$INSTALL_HOME/.bash_profile"
grep -Fq "$INSTALL_PREFIX/bin" "$INSTALL_HOME/.bashrc"
grep -Fq "$INSTALL_PREFIX/bin" "$INSTALL_HOME/.config/fish/config.fish"

run_with_timeout 180 "interactive smart bundled install" \
  env HOME="$SMART_HOME" SHELL=/bin/zsh \
    BSJ_INSTALL_PREFIX="$SMART_PREFIX" \
    BSJ_INSTALLER_ACTION_SELECTION="1" \
    BSJ_INSTALLER_POST_INSTALL_SELECTION="7" \
    script -q "$SMART_LOG" "$BUNDLE_DIR/install.sh" >/dev/null 2>&1
"$SMART_PREFIX/bin/bsj" --help >/dev/null
assert_log_contains "$SMART_LOG" "Choose what to do:"
assert_log_contains "$SMART_LOG" "Install / Update (Recommended smart mode)"
assert_log_contains "$SMART_LOG" "Installer auto-select: 1"
assert_log_contains "$SMART_LOG" "Smart mode selected bundled prebuilt install from this folder"
assert_log_contains "$SMART_LOG" "Plan: fetch or use a signed release bundle, install bsj, repair PATH, then offer a menu-first launch."
assert_log_contains "$SMART_LOG" "Installer auto-select: 7"
grep -Fq "$SMART_PREFIX/bin" "$SMART_HOME/.zprofile"

run_with_timeout 180 "post-install menu launch help" \
  env HOME="$MENU_LAUNCH_HOME" SHELL=/bin/zsh \
    BSJ_INSTALLER_POST_INSTALL_SELECTION="1" \
    BSJ_INSTALLER_LAUNCH_MODE="help" \
    script -q "$MENU_LAUNCH_LOG" "$BUNDLE_DIR/install.sh" --prebuilt --prefix "$MENU_LAUNCH_PREFIX" >/dev/null 2>&1
"$MENU_LAUNCH_PREFIX/bin/bsj" --help >/dev/null
assert_log_contains "$MENU_LAUNCH_LOG" "BlueScreen Journal installer menu"
assert_log_contains "$MENU_LAUNCH_LOG" "Installer auto-select: 1"
assert_log_contains "$MENU_LAUNCH_LOG" "Launch BlueScreen Journal here now (recommended)"
assert_log_contains "$MENU_LAUNCH_LOG" "Usage: bsj"
grep -Fq "$MENU_LAUNCH_PREFIX/bin" "$MENU_LAUNCH_HOME/.zprofile"

run_with_timeout 180 "same-terminal installer handoff" \
  env HOME="$MENU_LAUNCH_HOME" SHELL=/bin/zsh \
    BSJ_INSTALLER_POST_INSTALL_SELECTION="1" \
    BSJ_INSTALLER_LAUNCH_MODE="smoke-app" \
    BSJ_INSTALLER_LAUNCH_STYLE="same-terminal" \
    script -q "$HANDOFF_LOG" "$BUNDLE_DIR/install.sh" --prebuilt --prefix "$MENU_LAUNCH_PREFIX" >/dev/null 2>&1
assert_log_contains "$HANDOFF_LOG" "Preparing this terminal for the full-screen journal"
assert_log_contains "$HANDOFF_LOG" "[1/3] Resetting terminal input state"
assert_log_contains "$HANDOFF_LOG" "[2/3] Handing off to bsj"
assert_log_contains "$HANDOFF_LOG" "[3/3] Opening the blue-screen workspace"
assert_log_contains "$HANDOFF_LOG" "bsj "

run_with_timeout 180 "installer post-install utility menu" \
  env HOME="$MENU_HOME" SHELL=/bin/zsh \
    BSJ_INSTALLER_POST_INSTALL_SELECTION="4,6,5,7" \
    BSJ_INSTALLER_LAUNCH_MODE="help" \
    script -q "$MENU_LOG" "$BUNDLE_DIR/install.sh" --prebuilt --prefix "$MENU_PREFIX" >/dev/null 2>&1
"$MENU_PREFIX/bin/bsj" --help >/dev/null
assert_log_contains "$MENU_LOG" "BlueScreen Journal installer menu"
assert_log_contains "$MENU_LOG" "Print first-two-minutes cheat sheet"
assert_log_contains "$MENU_LOG" "BlueScreen Journal Cheat Sheet"
assert_log_contains "$MENU_LOG" "If you only remember three things"
assert_log_contains "$MENU_LOG" "Installer auto-select: 4"
assert_log_contains "$MENU_LOG" "Installer auto-select: 6"
assert_log_contains "$MENU_LOG" "Installer auto-select: 5"
assert_log_contains "$MENU_LOG" "Installer auto-select: 7"
assert_log_contains "$MENU_LOG" "Open BlueScreen Journal in a new Terminal window"
assert_log_contains "$MENU_LOG" "Usage: bsj"
assert_log_contains "$MENU_LOG" "BlueScreen Journal Doctor"
assert_log_contains "$MENU_LOG" "Summary: OK"
grep -Fq "$MENU_PREFIX/bin" "$MENU_HOME/.zprofile"

run_with_timeout 180 "bootstrap prebuilt install with archive" \
  env HOME="$BOOTSTRAP_HOME" SHELL=/bin/zsh \
    bash -lc "bash -s -- --prebuilt --archive '$ARCHIVE' --prefix '$BOOTSTRAP_PREFIX' < '$ROOT_DIR/install.sh'"
"$BOOTSTRAP_PREFIX/bin/bsj" --help >/dev/null
test -f "$BOOTSTRAP_PREFIX/share/doc/bsj/README.md"
test -f "$BOOTSTRAP_PREFIX/share/doc/bsj/CHANGELOG.md"
test -f "$BOOTSTRAP_PREFIX/share/doc/bsj/SUPPORT.md"
test -f "$BOOTSTRAP_PREFIX/share/doc/bsj/SECURITY.md"
test -f "$BOOTSTRAP_PREFIX/share/doc/bsj/CONTRIBUTING.md"
test -f "$BOOTSTRAP_PREFIX/share/doc/bsj/ROADMAP.md"
test -f "$BOOTSTRAP_PREFIX/share/doc/bsj/docs/TROUBLESHOOTING.md"
test -f "$BOOTSTRAP_PREFIX/share/doc/bsj/docs/SYNC_GUIDE.md"
test -f "$BOOTSTRAP_PREFIX/share/doc/bsj/docs/BACKUP_RESTORE.md"
test -f "$BOOTSTRAP_PREFIX/share/doc/bsj/docs/MACRO_GUIDE.md"
test -f "$BOOTSTRAP_PREFIX/share/doc/bsj/docs/TERMINAL_GUIDE.md"
test -f "$BOOTSTRAP_PREFIX/share/doc/bsj/docs/PRIVACY.md"
test -f "$BOOTSTRAP_PREFIX/share/doc/bsj/docs/assets/bsj-hero.gif"
test -f "$BOOTSTRAP_PREFIX/share/man/man1/bsj.1"
grep -Fq "$BOOTSTRAP_PREFIX/bin" "$BOOTSTRAP_HOME/.zprofile"
grep -Fq "$BOOTSTRAP_PREFIX/bin" "$BOOTSTRAP_HOME/.zshrc"
grep -Fq "$BOOTSTRAP_PREFIX/bin" "$BOOTSTRAP_HOME/.bash_profile"
grep -Fq "$BOOTSTRAP_PREFIX/bin" "$BOOTSTRAP_HOME/.bashrc"
grep -Fq "$BOOTSTRAP_PREFIX/bin" "$BOOTSTRAP_HOME/.config/fish/config.fish"

# Regression guard: exercise bootstrap install with no forwarded install args.
mkdir -p "$BOOTSTRAP_NOARGS_HOME"
run_with_timeout 180 "bootstrap prebuilt install without forwarded args" \
  env PATH="$SMOKE_PATH" HOME="$BOOTSTRAP_NOARGS_HOME" SHELL=/bin/zsh \
    bash -lc "bash -s -- --prebuilt --archive '$ARCHIVE' < '$ROOT_DIR/install.sh'"
"$BOOTSTRAP_NOARGS_HOME/.local/bin/bsj" --help >/dev/null
test -f "$BOOTSTRAP_NOARGS_HOME/.local/share/doc/bsj/README.md"
test -f "$BOOTSTRAP_NOARGS_HOME/.local/share/man/man1/bsj.1"
grep -Fq "$BOOTSTRAP_NOARGS_HOME/.local/bin" "$BOOTSTRAP_NOARGS_HOME/.zprofile"
grep -Fq "$BOOTSTRAP_NOARGS_HOME/.local/bin" "$BOOTSTRAP_NOARGS_HOME/.zshrc"
grep -Fq "$BOOTSTRAP_NOARGS_HOME/.local/bin" "$BOOTSTRAP_NOARGS_HOME/.bash_profile"
grep -Fq "$BOOTSTRAP_NOARGS_HOME/.local/bin" "$BOOTSTRAP_NOARGS_HOME/.bashrc"
grep -Fq "$BOOTSTRAP_NOARGS_HOME/.local/bin" "$BOOTSTRAP_NOARGS_HOME/.config/fish/config.fish"

# Bash profile fallback coverage.
mkdir -p "$BOOTSTRAP_BASH_HOME"
run_with_timeout 180 "bootstrap prebuilt install with bash shell" \
  env PATH="$SMOKE_PATH" HOME="$BOOTSTRAP_BASH_HOME" SHELL=/bin/bash \
    bash -lc "bash -s -- --prebuilt --archive '$ARCHIVE' < '$ROOT_DIR/install.sh'"
"$BOOTSTRAP_BASH_HOME/.local/bin/bsj" --help >/dev/null
grep -Fq "$BOOTSTRAP_BASH_HOME/.local/bin" "$BOOTSTRAP_BASH_HOME/.bash_profile"
grep -Fq "$BOOTSTRAP_BASH_HOME/.local/bin" "$BOOTSTRAP_BASH_HOME/.bashrc"
grep -Fq "$BOOTSTRAP_BASH_HOME/.local/bin" "$BOOTSTRAP_BASH_HOME/.zprofile"
grep -Fq "$BOOTSTRAP_BASH_HOME/.local/bin" "$BOOTSTRAP_BASH_HOME/.zshrc"
grep -Fq "$BOOTSTRAP_BASH_HOME/.local/bin" "$BOOTSTRAP_BASH_HOME/.config/fish/config.fish"

# Regression guard: existing install + smart mode should stay in one installer flow while source-updating.
mkdir -p "$SOURCE_UPDATE_HOME"
run_with_timeout 900 "smart mode source update with existing install" \
  env PATH="$INSTALL_PREFIX/bin:$SMOKE_PATH" HOME="$SOURCE_UPDATE_HOME" SHELL=/bin/zsh \
    BSJ_INSTALL_SOURCE_DIR="$ROOT_DIR" \
    bash -lc "bash -s -- --prefix '$SOURCE_UPDATE_PREFIX' < '$ROOT_DIR/install.sh'" \
    >"$SOURCE_UPDATE_LOG" 2>&1
"$SOURCE_UPDATE_PREFIX/bin/bsj" --help >/dev/null
assert_log_contains "$SOURCE_UPDATE_LOG" "Smart mode selected source update from latest main because bsj is already installed"
assert_log_contains "$SOURCE_UPDATE_LOG" "Using provided source tree override: $ROOT_DIR"
assert_log_contains "$SOURCE_UPDATE_LOG" "Source update archive is ready."
assert_log_contains "$SOURCE_UPDATE_LOG" "Continuing in this installer window to build the source tree."
assert_log_contains "$SOURCE_UPDATE_LOG" "Next step: cargo install --path $ROOT_DIR --locked --force"
assert_log_contains "$SOURCE_UPDATE_LOG" "Installing bsj from source into $SOURCE_UPDATE_PREFIX/bin"
assert_log_contains "$SOURCE_UPDATE_LOG" "State: building and installing from source"
assert_log_contains "$SOURCE_UPDATE_LOG" "State: checking PATH integration"
assert_log_contains "$SOURCE_UPDATE_LOG" "State: verifying installed binary"
grep -Fq "$SOURCE_UPDATE_PREFIX/bin" "$SOURCE_UPDATE_HOME/.zprofile"

# Regression guard: existing install + choosing Install / Update from the top installer menu should also source-update cleanly.
run_with_timeout 900 "interactive installer update action with existing install" \
  env PATH="$INSTALL_PREFIX/bin:$SMOKE_PATH" HOME="$UPDATE_ACTION_HOME" SHELL=/bin/zsh \
    BSJ_INSTALL_SOURCE_DIR="$ROOT_DIR" \
    BSJ_INSTALLER_ACTION_SELECTION="1" \
    BSJ_INSTALLER_POST_INSTALL_SELECTION="7" \
    script -q "$UPDATE_ACTION_LOG" /bin/bash -lc "bash -s -- < '$ROOT_DIR/install.sh'" >/dev/null 2>&1
"$UPDATE_ACTION_HOME/.cargo/bin/bsj" --help >/dev/null
assert_log_contains "$UPDATE_ACTION_LOG" "Choose what to do:"
assert_log_contains "$UPDATE_ACTION_LOG" "Installer auto-select: 1"
assert_log_contains "$UPDATE_ACTION_LOG" "Install / Update (Recommended smart mode)"
assert_log_contains "$UPDATE_ACTION_LOG" "Smart mode selected source update from latest main because bsj is already installed"
assert_log_contains "$UPDATE_ACTION_LOG" "Using provided source tree override: $ROOT_DIR"
assert_log_contains "$UPDATE_ACTION_LOG" "Continuing in this installer window to build the source tree."
assert_log_contains "$UPDATE_ACTION_LOG" "Installer auto-select: 7"
assert_log_contains "$UPDATE_ACTION_LOG" "State: opening post-install options"
grep -Fq "$UPDATE_ACTION_HOME/.cargo/bin" "$UPDATE_ACTION_HOME/.zprofile"

cat <<EOF
Smoke test passed:
  Archive: $ARCHIVE
  Bundle:  $BUNDLE_DIR
  Prefix:  $INSTALL_PREFIX
  Bootstrap Prefix: $BOOTSTRAP_PREFIX
  Source Update Prefix: $SOURCE_UPDATE_PREFIX
  Interactive Update Prefix: $UPDATE_ACTION_HOME/.cargo
EOF
