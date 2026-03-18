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
  ARCHIVE="$(find "$ROOT_DIR/dist" -maxdepth 1 -type f -name '*.tar.gz' | sort | tail -n 1)"
fi

[[ -f "$ARCHIVE" ]] || { echo "Archive not found: $ARCHIVE" >&2; exit 1; }

TMP_DIR="$(mktemp -d /tmp/bsj-dist-smoke.XXXXXX)"
INSTALL_PREFIX="$TMP_DIR/install-root"
BOOTSTRAP_PREFIX="$TMP_DIR/bootstrap-root"
BOOTSTRAP_NOARGS_HOME="$TMP_DIR/bootstrap-home-noargs"

tar -C "$TMP_DIR" -xzf "$ARCHIVE"
BUNDLE_DIR="$(find "$TMP_DIR" -mindepth 1 -maxdepth 1 -type d | head -n 1)"
[[ -d "$BUNDLE_DIR" ]] || { echo "Bundle directory not found after extraction" >&2; exit 1; }

"$BUNDLE_DIR/install.sh" --prebuilt --prefix "$INSTALL_PREFIX"
"$INSTALL_PREFIX/bin/bsj" --help >/dev/null
"$INSTALL_PREFIX/bin/bsj" guide setup >/dev/null
"$INSTALL_PREFIX/bin/bsj" guide distribution >/dev/null
"$INSTALL_PREFIX/bin/bsj" settings >/dev/null
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

cat "$ROOT_DIR/install.sh" | bash -s -- --prebuilt --archive "$ARCHIVE" --prefix "$BOOTSTRAP_PREFIX"
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

# Regression guard: exercise bootstrap install with no forwarded install args.
mkdir -p "$BOOTSTRAP_NOARGS_HOME"
cat "$ROOT_DIR/install.sh" | HOME="$BOOTSTRAP_NOARGS_HOME" bash -s -- --prebuilt --archive "$ARCHIVE"
"$BOOTSTRAP_NOARGS_HOME/.local/bin/bsj" --help >/dev/null
test -f "$BOOTSTRAP_NOARGS_HOME/.local/share/doc/bsj/README.md"
test -f "$BOOTSTRAP_NOARGS_HOME/.local/share/man/man1/bsj.1"

cat <<EOF
Smoke test passed:
  Archive: $ARCHIVE
  Bundle:  $BUNDLE_DIR
  Prefix:  $INSTALL_PREFIX
  Bootstrap Prefix: $BOOTSTRAP_PREFIX
EOF
