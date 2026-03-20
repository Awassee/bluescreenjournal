#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

REPO="Awassee/bluescreenjournal"
REF="main"
VERSION=""
REPORT=""
KEEP_TEMP=0
declare -a QA_NOTES=()

usage() {
  cat <<'EOF'
certify-release.sh

Runs a clean-account release certification pass against the public installer and
writes a Markdown certification report.

Usage:
  ./scripts/certify-release.sh [--repo OWNER/REPO] [--ref REF] [--version TAG] [--report PATH] [--qa-note TEXT] [--keep-temp]

Options:
  --repo OWNER/REPO  GitHub repository to test. Default: Awassee/bluescreenjournal
  --ref REF          Git ref used for the raw installer fetch. Default: main
  --version TAG      Release tag to install. Defaults to Cargo version with leading v.
  --report PATH      Markdown report path. Default: artifacts/release-certification/<tag>.md
  --qa-note TEXT     Add a Release QA Notes bullet to the report. Repeatable.
  --keep-temp        Keep the certification temp directory for inspection.
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
    --report)
      [[ $# -ge 2 ]] || { echo "--report requires a value" >&2; exit 1; }
      REPORT="$2"
      shift 2
      ;;
    --qa-note)
      [[ $# -ge 2 ]] || { echo "--qa-note requires a value" >&2; exit 1; }
      QA_NOTES+=("$2")
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
  VERSION="v$(awk -F'\"' '/^version = \"/ { print $2; exit }' Cargo.toml)"
fi

if [[ -z "$REPORT" ]]; then
  REPORT="$ROOT_DIR/artifacts/release-certification/${VERSION}.md"
fi

REPORT_PATH="$REPORT"
OUTPUT_DIR="$(dirname "$REPORT_PATH")"
mkdir -p "$OUTPUT_DIR"

TMP_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/bsj-certify.XXXXXX")"
PREFIX_DIR="$TMP_ROOT/prefix"
PUBLIC_LOG="$OUTPUT_DIR/public-install.log"
HELP_LOG="$OUTPUT_DIR/bsj-help.txt"
CHEAT_LOG="$OUTPUT_DIR/guide-cheatsheet.txt"
QUICKSTART_LOG="$OUTPUT_DIR/guide-quickstart.txt"

cleanup() {
  if [[ "$KEEP_TEMP" -eq 0 ]]; then
    rm -rf "$TMP_ROOT"
  fi
}
trap cleanup EXIT

echo "==> Release certification"
echo "    Repo:    $REPO"
echo "    Ref:     $REF"
echo "    Version: $VERSION"
echo "    Report:  $REPORT_PATH"

./scripts/smoke-public-install.sh \
  --repo "$REPO" \
  --ref "$REF" \
  --version "$VERSION" \
  --prefix "$PREFIX_DIR" | tee "$PUBLIC_LOG"

"$PREFIX_DIR/bin/bsj" --help > "$HELP_LOG"
"$PREFIX_DIR/bin/bsj" guide cheatsheet > "$CHEAT_LOG"
"$PREFIX_DIR/bin/bsj" guide quickstart > "$QUICKSTART_LOG"

grep -F "bsj guide cheatsheet" "$HELP_LOG" >/dev/null
grep -F "BlueScreen Journal Cheat Sheet" "$CHEAT_LOG" >/dev/null
grep -F "If you only remember three things" "$CHEAT_LOG" >/dev/null
grep -F "BlueScreen Journal Quickstart" "$QUICKSTART_LOG" >/dev/null

timestamp="$(date '+%Y-%m-%d %H:%M:%S %Z')"
installed_version="$("$PREFIX_DIR/bin/bsj" --version)"

{
  echo "# Release Certification ${VERSION}"
  echo
  echo "- Date: ${timestamp}"
  echo "- Scope: clean-account public installer certification"
  echo "- Repo: \`${REPO}\`"
  echo "- Ref: \`${REF}\`"
  echo "- Installed version: \`${installed_version}\`"
  echo
  echo "## Checks passed"
  echo
  echo "- Raw GitHub installer fetched and completed successfully."
  echo "- Installed binary reported the expected version."
  echo "- \`bsj --help\` exposed the first-run guide surfaces, including \`bsj guide cheatsheet\`."
  echo "- \`bsj guide cheatsheet\` rendered the short first-two-minutes guidance."
  echo "- \`bsj guide quickstart\` rendered the longer day-one walkthrough."
  echo
  echo "## Artifacts"
  echo
  echo "- Public installer log: \`$(basename "$PUBLIC_LOG")\`"
  echo "- Help output: \`$(basename "$HELP_LOG")\`"
  echo "- Cheat sheet output: \`$(basename "$CHEAT_LOG")\`"
  echo "- Quickstart output: \`$(basename "$QUICKSTART_LOG")\`"
  if [[ "$KEEP_TEMP" -eq 1 ]]; then
    echo "- Temp root kept for inspection: \`${TMP_ROOT}\`"
  fi
  echo
  echo "## Release QA Notes"
  echo
  if [[ "${#QA_NOTES[@]}" -eq 0 ]]; then
    echo "- none"
  else
    for note in "${QA_NOTES[@]}"; do
      echo "- ${note}"
    done
  fi
} > "$REPORT_PATH"

echo "Release certification report written to $REPORT_PATH"
