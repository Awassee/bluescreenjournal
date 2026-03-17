#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BUNDLE_DIR=""
BINARY_PATH=""

usage() {
  cat <<'EOF'
audit-release.sh

Usage:
  ./scripts/audit-release.sh [--bundle PATH] [--binary PATH]

Checks the repo and release artifacts for common secrets and private path leakage.
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --help|-h)
      usage
      exit 0
      ;;
    --bundle)
      [[ $# -ge 2 ]] || { echo "--bundle requires a value" >&2; exit 1; }
      BUNDLE_DIR="$2"
      shift 2
      ;;
    --binary)
      [[ $# -ge 2 ]] || { echo "--binary requires a value" >&2; exit 1; }
      BINARY_PATH="$2"
      shift 2
      ;;
    *)
      echo "Unknown option: $1" >&2
      exit 1
      ;;
  esac
done

echo "Auditing repo text files..."
if rg -n -P --hidden \
  --glob '!target/**' \
  --glob '!.git/**' \
  --glob '!dist/**' \
  --glob '!docs/config.example.json' \
  --glob '!scripts/audit-release.sh' \
  '(AKIA[0-9A-Z]{16}|ASIA[0-9A-Z]{16}|ghp_[A-Za-z0-9]{36}|github_pat_[A-Za-z0-9_]{20,}|sk-[A-Za-z0-9]{20,}|BEGIN [A-Z ]*PRIVATE KEY|@[A-Za-z0-9.-]+\.local|pinehollow|/Users/(?!you\b)[A-Za-z0-9._-]+|sean-mba-m2-2071)' \
  "$ROOT_DIR"; then
  echo "Repo audit failed" >&2
  exit 1
fi

if [[ -n "$BUNDLE_DIR" ]]; then
  [[ -d "$BUNDLE_DIR" ]] || { echo "Bundle not found: $BUNDLE_DIR" >&2; exit 1; }

  echo "Auditing bundle text files..."
  if rg -n -P \
    --glob '!config.example.json' \
    '(@[A-Za-z0-9.-]+\.local|pinehollow|/Users/(?!you\b)[A-Za-z0-9._-]+|/home/[^/]+/|/private/tmp/rust-|sean-mba-m2-2071)' \
    "$BUNDLE_DIR/README.md" "$BUNDLE_DIR/docs" "$BUNDLE_DIR/install.sh" "$BUNDLE_DIR/packaging"; then
    echo "Bundle text audit failed" >&2
    exit 1
  fi

  if [[ -z "$BINARY_PATH" ]]; then
    BINARY_PATH="$BUNDLE_DIR/bin/bsj"
  fi
fi

if [[ -n "$BINARY_PATH" ]]; then
  [[ -f "$BINARY_PATH" ]] || { echo "Binary not found: $BINARY_PATH" >&2; exit 1; }

  echo "Auditing binary strings..."
  if strings "$BINARY_PATH" | rg -q '(/Users/|/home/[^/]+/|/private/tmp/rust-|\.cargo/registry/src/|@[A-Za-z0-9.-]+\.local|pinehollow|sean-mba-m2-2071)'; then
    echo "Binary audit failed" >&2
    exit 1
  fi
fi

echo "Audit passed."
