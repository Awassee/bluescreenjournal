#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
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
SMOKE_PATH="/usr/bin:/bin:/usr/sbin:/sbin"
INSTALL_LOG="$TMP_DIR/install-prebuilt.log"
SMART_LOG="$TMP_DIR/install-smart.log"
MENU_LOG="$TMP_DIR/install-prebuilt-menu.log"
MENU_LAUNCH_LOG="$TMP_DIR/install-prebuilt-menu-launch.log"
HANDOFF_LOG="$TMP_DIR/install-handoff.log"

tar -C "$TMP_DIR" -xzf "$ARCHIVE"
BUNDLE_DIR="$(find "$TMP_DIR" -mindepth 1 -maxdepth 1 -type d | head -n 1)"
[[ -d "$BUNDLE_DIR" ]] || { echo "Bundle directory not found after extraction" >&2; exit 1; }

HOME="$INSTALL_HOME" SHELL=/bin/zsh "$BUNDLE_DIR/install.sh" --prebuilt --prefix "$INSTALL_PREFIX" | tee "$INSTALL_LOG"
"$INSTALL_PREFIX/bin/bsj" --help >/dev/null
"$INSTALL_PREFIX/bin/bsj" guide setup >/dev/null
"$INSTALL_PREFIX/bin/bsj" guide distribution >/dev/null
"$INSTALL_PREFIX/bin/bsj" settings >/dev/null
grep -Fq "First-run flow:" "$INSTALL_LOG"
grep -Fq "Press F2 to save, or type **save** then Enter for quick-save + next entry" "$INSTALL_LOG"
grep -Fq "Press Esc (or Ctrl+O) to open menus" "$INSTALL_LOG"
grep -Fq "Use SETUP -> Cloud Provider Setup for folder sync or direct cloud connectors" "$INSTALL_LOG"
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

HOME="$SMART_HOME" SHELL=/bin/zsh \
  BSJ_INSTALL_PREFIX="$SMART_PREFIX" \
  BSJ_INSTALLER_ACTION_SELECTION="1" \
  BSJ_INSTALLER_POST_INSTALL_SELECTION="7" \
  script -q "$SMART_LOG" "$BUNDLE_DIR/install.sh" >/dev/null
"$SMART_PREFIX/bin/bsj" --help >/dev/null
grep -Fq "Choose what to do:" "$SMART_LOG"
grep -Fq "Install / Update (Recommended smart mode)" "$SMART_LOG"
grep -Fq "Installer auto-select: 1" "$SMART_LOG"
grep -Fq "Smart mode selected bundled prebuilt install from this folder" "$SMART_LOG"
grep -Fq "Plan: fetch or use a signed release bundle, install bsj, repair PATH, then offer a menu-first launch." "$SMART_LOG"
grep -Fq "Installer auto-select: 7" "$SMART_LOG"
grep -Fq "$SMART_PREFIX/bin" "$SMART_HOME/.zprofile"

HOME="$MENU_LAUNCH_HOME" SHELL=/bin/zsh \
  BSJ_INSTALLER_POST_INSTALL_SELECTION="1" \
  BSJ_INSTALLER_LAUNCH_MODE="help" \
  script -q "$MENU_LAUNCH_LOG" "$BUNDLE_DIR/install.sh" --prebuilt --prefix "$MENU_LAUNCH_PREFIX" >/dev/null
"$MENU_LAUNCH_PREFIX/bin/bsj" --help >/dev/null
grep -Fq "BlueScreen Journal installer menu" "$MENU_LAUNCH_LOG"
grep -Fq "Installer auto-select: 1" "$MENU_LAUNCH_LOG"
grep -Fq "Launch BlueScreen Journal here now (recommended)" "$MENU_LAUNCH_LOG"
grep -Fq "Usage: bsj" "$MENU_LAUNCH_LOG"
grep -Fq "$MENU_LAUNCH_PREFIX/bin" "$MENU_LAUNCH_HOME/.zprofile"

HOME="$MENU_LAUNCH_HOME" SHELL=/bin/zsh \
  BSJ_INSTALLER_POST_INSTALL_SELECTION="1" \
  BSJ_INSTALLER_LAUNCH_MODE="smoke-app" \
  BSJ_INSTALLER_LAUNCH_STYLE="same-terminal" \
  script -q "$HANDOFF_LOG" "$BUNDLE_DIR/install.sh" --prebuilt --prefix "$MENU_LAUNCH_PREFIX" >/dev/null
grep -Fq "Preparing this terminal for the full-screen journal" "$HANDOFF_LOG"
grep -Fq "[1/3] Resetting terminal input state" "$HANDOFF_LOG"
grep -Fq "[2/3] Handing off to bsj" "$HANDOFF_LOG"
grep -Fq "[3/3] Opening the blue-screen workspace" "$HANDOFF_LOG"
grep -Fq "bsj " "$HANDOFF_LOG"

HOME="$MENU_HOME" SHELL=/bin/zsh \
  BSJ_INSTALLER_POST_INSTALL_SELECTION="4,6,5,7" \
  BSJ_INSTALLER_LAUNCH_MODE="help" \
  script -q "$MENU_LOG" "$BUNDLE_DIR/install.sh" --prebuilt --prefix "$MENU_PREFIX" >/dev/null
"$MENU_PREFIX/bin/bsj" --help >/dev/null
grep -Fq "BlueScreen Journal installer menu" "$MENU_LOG"
grep -Fq "Print quickstart + key cheat sheet" "$MENU_LOG"
grep -Fq "BlueScreen Journal Quickstart" "$MENU_LOG"
grep -Fq "Installer auto-select: 4" "$MENU_LOG"
grep -Fq "Installer auto-select: 6" "$MENU_LOG"
grep -Fq "Installer auto-select: 5" "$MENU_LOG"
grep -Fq "Installer auto-select: 7" "$MENU_LOG"
grep -Fq "Open BlueScreen Journal in a new Terminal window" "$MENU_LOG"
grep -Fq "Usage: bsj" "$MENU_LOG"
grep -Fq "BlueScreen Journal Doctor" "$MENU_LOG"
grep -Fq "Summary: OK" "$MENU_LOG"
grep -Fq "$MENU_PREFIX/bin" "$MENU_HOME/.zprofile"

HOME="$BOOTSTRAP_HOME" SHELL=/bin/zsh bash -s -- --prebuilt --archive "$ARCHIVE" --prefix "$BOOTSTRAP_PREFIX" < "$ROOT_DIR/install.sh"
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
PATH="$SMOKE_PATH" HOME="$BOOTSTRAP_NOARGS_HOME" SHELL=/bin/zsh bash -s -- --prebuilt --archive "$ARCHIVE" < "$ROOT_DIR/install.sh"
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
PATH="$SMOKE_PATH" HOME="$BOOTSTRAP_BASH_HOME" SHELL=/bin/bash bash -s -- --prebuilt --archive "$ARCHIVE" < "$ROOT_DIR/install.sh"
"$BOOTSTRAP_BASH_HOME/.local/bin/bsj" --help >/dev/null
grep -Fq "$BOOTSTRAP_BASH_HOME/.local/bin" "$BOOTSTRAP_BASH_HOME/.bash_profile"
grep -Fq "$BOOTSTRAP_BASH_HOME/.local/bin" "$BOOTSTRAP_BASH_HOME/.bashrc"
grep -Fq "$BOOTSTRAP_BASH_HOME/.local/bin" "$BOOTSTRAP_BASH_HOME/.zprofile"
grep -Fq "$BOOTSTRAP_BASH_HOME/.local/bin" "$BOOTSTRAP_BASH_HOME/.zshrc"
grep -Fq "$BOOTSTRAP_BASH_HOME/.local/bin" "$BOOTSTRAP_BASH_HOME/.config/fish/config.fish"

cat <<EOF
Smoke test passed:
  Archive: $ARCHIVE
  Bundle:  $BUNDLE_DIR
  Prefix:  $INSTALL_PREFIX
  Bootstrap Prefix: $BOOTSTRAP_PREFIX
EOF
