#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUTPUT_DIR="$ROOT_DIR/dist"
TARGET=""

usage() {
  cat <<'EOF'
package-release.sh

Usage:
  ./scripts/package-release.sh [--target TRIPLE] [--output-dir PATH]

Examples:
  ./scripts/package-release.sh
  ./scripts/package-release.sh --target aarch64-apple-darwin
  ./scripts/package-release.sh --output-dir /tmp/bsj-dist
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --help|-h)
      usage
      exit 0
      ;;
    --target)
      [[ $# -ge 2 ]] || { echo "--target requires a value" >&2; exit 1; }
      TARGET="$2"
      shift 2
      ;;
    --output-dir)
      [[ $# -ge 2 ]] || { echo "--output-dir requires a value" >&2; exit 1; }
      OUTPUT_DIR="$2"
      shift 2
      ;;
    *)
      echo "Unknown option: $1" >&2
      exit 1
      ;;
  esac
done

NAME="$(awk -F ' *= *' '$1=="name"{gsub(/"/,"",$2); print $2; exit}' "$ROOT_DIR/Cargo.toml")"
VERSION="$(awk -F ' *= *' '$1=="version"{gsub(/"/,"",$2); print $2; exit}' "$ROOT_DIR/Cargo.toml")"
HOST_TARGET="$(rustc -vV | awk '/^host: / {print $2}')"
BUILD_TARGET="${TARGET:-$HOST_TARGET}"

if [[ -n "$TARGET" ]]; then
  (
    cd "$ROOT_DIR"
    cargo build --release --locked --target "$TARGET"
  )
  BINARY_PATH="$ROOT_DIR/target/$TARGET/release/$NAME"
else
  (
    cd "$ROOT_DIR"
    cargo build --release --locked
  )
  BINARY_PATH="$ROOT_DIR/target/release/$NAME"
fi

[[ -x "$BINARY_PATH" ]] || { echo "Built binary not found: $BINARY_PATH" >&2; exit 1; }

BUNDLE_NAME="${NAME}-${VERSION}-${BUILD_TARGET}"
BUNDLE_DIR="$OUTPUT_DIR/$BUNDLE_NAME"
ARCHIVE_PATH="$OUTPUT_DIR/${BUNDLE_NAME}.tar.gz"
CHECKSUM_PATH="${ARCHIVE_PATH}.sha256"
FORMULA_TEMPLATE="$ROOT_DIR/packaging/homebrew/${NAME}.rb.template"

rm -rf "$BUNDLE_DIR"
mkdir -p \
  "$BUNDLE_DIR/bin" \
  "$BUNDLE_DIR/docs" \
  "$BUNDLE_DIR/completions/bash" \
  "$BUNDLE_DIR/completions/zsh" \
  "$BUNDLE_DIR/completions/fish" \
  "$BUNDLE_DIR/packaging/homebrew" \
  "$OUTPUT_DIR"

install -m 755 "$BINARY_PATH" "$BUNDLE_DIR/bin/$NAME"
install -m 755 "$ROOT_DIR/install.sh" "$BUNDLE_DIR/install.sh"
install -m 644 "$ROOT_DIR/README.md" "$BUNDLE_DIR/README.md"
find "$ROOT_DIR/docs" -maxdepth 1 -type f -exec install -m 644 {} "$BUNDLE_DIR/docs" \;
install -m 644 "$FORMULA_TEMPLATE" "$BUNDLE_DIR/packaging/homebrew/${NAME}.rb.template"
printf '%s\n' "$VERSION" > "$BUNDLE_DIR/VERSION"
"$BINARY_PATH" completions bash > "$BUNDLE_DIR/completions/bash/$NAME"
"$BINARY_PATH" completions zsh > "$BUNDLE_DIR/completions/zsh/_$NAME"
"$BINARY_PATH" completions fish > "$BUNDLE_DIR/completions/fish/$NAME.fish"

rm -f "$ARCHIVE_PATH" "$CHECKSUM_PATH"
tar -C "$OUTPUT_DIR" -czf "$ARCHIVE_PATH" "$BUNDLE_NAME"
SHA256="$(shasum -a 256 "$ARCHIVE_PATH" | awk '{print $1}')"
printf '%s  %s\n' "$SHA256" "$(basename "$ARCHIVE_PATH")" > "$CHECKSUM_PATH"

sed \
  -e "s#@HOMEPAGE@#REPLACE_WITH_PROJECT_HOMEPAGE#g" \
  -e "s#@URL@#REPLACE_WITH_RELEASE_URL/$(basename "$ARCHIVE_PATH")#g" \
  -e "s#@VERSION@#$VERSION#g" \
  -e "s#@SHA256@#$SHA256#g" \
  "$FORMULA_TEMPLATE" > "$BUNDLE_DIR/packaging/homebrew/${NAME}.rb"

cat <<EOF
Release bundle ready:
  Bundle:   $BUNDLE_DIR
  Archive:  $ARCHIVE_PATH
  SHA256:   $CHECKSUM_PATH
  Formula:  $BUNDLE_DIR/packaging/homebrew/${NAME}.rb
EOF
