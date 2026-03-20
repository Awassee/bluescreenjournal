#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

REPO="Awassee/bluescreenjournal"
REF="main"
VERSION=""
PREFIX=""
KEEP_TEMP=0

usage() {
  cat <<'EOF'
smoke-public-install.sh

Validates the public one-line installer path against a GitHub repo/ref in a clean temp HOME.

Usage:
  ./scripts/smoke-public-install.sh [--repo OWNER/REPO] [--ref REF] [--version TAG] [--prefix PATH] [--keep-temp]

Options:
  --repo OWNER/REPO  GitHub repository to test. Default: Awassee/bluescreenjournal
  --ref REF          Git ref used for raw installer fetch. Default: main
  --version TAG      Release tag to install (for example: v2.1.0). Defaults to Cargo version.
  --prefix PATH      Install prefix for the smoke run. Defaults to a temp prefix.
  --keep-temp        Keep the temp HOME/prefix/logs for inspection.
  -h, --help         Show this help.
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --repo)
      [[ $# -ge 2 ]] || { echo "--repo requires a value" >&2; exit 1; }
      REPO="$2"
      shift 2
      ;;
    --ref)
      [[ $# -ge 2 ]] || { echo "--ref requires a value" >&2; exit 1; }
      REF="$2"
      shift 2
      ;;
    --version)
      [[ $# -ge 2 ]] || { echo "--version requires a value" >&2; exit 1; }
      VERSION="$2"
      shift 2
      ;;
    --prefix)
      [[ $# -ge 2 ]] || { echo "--prefix requires a value" >&2; exit 1; }
      PREFIX="$2"
      shift 2
      ;;
    --keep-temp)
      KEEP_TEMP=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

require_command() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "Missing required command: $1" >&2
    exit 1
  }
}

require_command bash
require_command curl
require_command grep
require_command mktemp

if [[ -z "$VERSION" ]]; then
  VERSION="v$(awk -F'"' '/^version = "/ { print $2; exit }' Cargo.toml)"
fi

EXPECTED_VERSION="${VERSION#v}"
TMP_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/bsj-public-smoke.XXXXXX")"
HOME_DIR="$TMP_ROOT/home"
PREFIX_DIR="${PREFIX:-$TMP_ROOT/prefix}"
LOG_PATH="$TMP_ROOT/public-install.log"
INSTALLER_URL="https://raw.githubusercontent.com/${REPO}/${REF}/install.sh"
SMOKE_PATH="/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin:${PATH:-}"

cleanup() {
  if [[ "$KEEP_TEMP" -eq 0 ]]; then
    rm -rf "$TMP_ROOT"
  fi
}
trap cleanup EXIT

mkdir -p "$HOME_DIR"

echo "==> Public installer smoke"
echo "    Repo:     $REPO"
echo "    Ref:      $REF"
echo "    Version:  $VERSION"
echo "    Prefix:   $PREFIX_DIR"
echo "    Installer: $INSTALLER_URL"

HOME="$HOME_DIR" SHELL=/bin/zsh PATH="$SMOKE_PATH" \
  bash -lc "curl -fsSL '$INSTALLER_URL' | bash -s -- --prebuilt --version '$VERSION' --prefix '$PREFIX_DIR'" \
  | tee "$LOG_PATH"

INSTALLED_VERSION="$("$PREFIX_DIR/bin/bsj" --version)"
[[ "$INSTALLED_VERSION" == "bsj $EXPECTED_VERSION" ]] || {
  echo "Unexpected installed version: $INSTALLED_VERSION" >&2
  exit 1
}

grep -F "Selected release asset:" "$LOG_PATH" >/dev/null
grep -F "Added $PREFIX_DIR/bin to PATH for this installer session." "$LOG_PATH" >/dev/null

if grep -F "Warning: Install finished, but this shell cannot find bsj yet" "$LOG_PATH" >/dev/null; then
  echo "Unexpected PATH warning present in public install log." >&2
  exit 1
fi

for target_file in \
  "$HOME_DIR/.zprofile" \
  "$HOME_DIR/.zshrc" \
  "$HOME_DIR/.bash_profile" \
  "$HOME_DIR/.bashrc" \
  "$HOME_DIR/.config/fish/config.fish"; do
  [[ -f "$target_file" ]] || {
    echo "Expected PATH file missing: $target_file" >&2
    exit 1
  }
done

echo "Public installer smoke passed:"
echo "  Version:  $INSTALLED_VERSION"
echo "  Log:      $LOG_PATH"
echo "  Temp root: $TMP_ROOT"
