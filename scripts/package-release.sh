#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUTPUT_DIR="$ROOT_DIR/dist"
TARGET=""
UNIVERSAL=0
AUDIT=1
PACKAGE_RUSTFLAGS="${RUSTFLAGS:-}"

bootstrap_rustup_toolchain_path() {
  local rustc_path=""
  local toolchain_bin=""

  if ! command -v rustup >/dev/null 2>&1; then
    return
  fi

  rustc_path="$(rustup which rustc 2>/dev/null || true)"
  if [[ -z "$rustc_path" ]]; then
    return
  fi

  toolchain_bin="$(dirname "$rustc_path")"
  case ":$PATH:" in
    *":$toolchain_bin:"*) ;;
    *)
      PATH="$toolchain_bin:$PATH"
      export PATH
      ;;
  esac
}

usage() {
  cat <<'EOF'
package-release.sh

Usage:
  ./scripts/package-release.sh [--target TRIPLE | --universal] [--output-dir PATH] [--skip-audit]

Examples:
  ./scripts/package-release.sh
  ./scripts/package-release.sh --target aarch64-apple-darwin
  ./scripts/package-release.sh --universal
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
    --universal)
      UNIVERSAL=1
      shift
      ;;
    --output-dir)
      [[ $# -ge 2 ]] || { echo "--output-dir requires a value" >&2; exit 1; }
      OUTPUT_DIR="$2"
      shift 2
      ;;
    --skip-audit)
      AUDIT=0
      shift
      ;;
    *)
      echo "Unknown option: $1" >&2
      exit 1
      ;;
  esac
done

bootstrap_rustup_toolchain_path

if [[ -n "$TARGET" && "$UNIVERSAL" -eq 1 ]]; then
  echo "--target and --universal cannot be combined" >&2
  exit 1
fi

append_rustflag() {
  if [[ -n "$PACKAGE_RUSTFLAGS" ]]; then
    PACKAGE_RUSTFLAGS+=" "
  fi
  PACKAGE_RUSTFLAGS+="$1"
}

ensure_target_available() {
  local target="$1"
  local target_libdir=""
  target_libdir="$(rustc --print target-libdir --target "$target" 2>/dev/null || true)"
  if [[ -n "$target_libdir" ]] && compgen -G "$target_libdir/libcore-*" >/dev/null; then
    return
  fi

  if command -v rustup >/dev/null 2>&1; then
    rustup target add "$target"
    return
  fi

  echo "target '$target' is not installed and rustup is unavailable" >&2
  exit 1
}

build_binary_for_target() {
  local target="$1"
  ensure_target_available "$target"
  (
    cd "$ROOT_DIR"
    RUSTFLAGS="$PACKAGE_RUSTFLAGS" cargo build --release --locked --target "$target"
  )
}

append_rustflag "--remap-path-prefix=$ROOT_DIR=/workspace"
if [[ -n "${HOME:-}" ]]; then
  append_rustflag "--remap-path-prefix=$HOME=/home/builder"
fi

NAME="$(awk -F ' *= *' '$1=="name"{gsub(/"/,"",$2); print $2; exit}' "$ROOT_DIR/Cargo.toml")"
VERSION="$(awk -F ' *= *' '$1=="version"{gsub(/"/,"",$2); print $2; exit}' "$ROOT_DIR/Cargo.toml")"
HOST_TARGET="$(rustc -vV | awk '/^host: / {print $2}')"
FORMULA_TEMPLATE="$ROOT_DIR/packaging/homebrew/${NAME}.rb.template"
SCRUB_SCRIPT="$ROOT_DIR/scripts/scrub-binary-paths.sh"
AUDIT_SCRIPT="$ROOT_DIR/scripts/audit-release.sh"

BINARY_PATH=""
BUILD_TARGET=""
declare -a BUNDLE_TARGETS=()

if [[ "$UNIVERSAL" -eq 1 ]]; then
  [[ "$(uname -s)" == "Darwin" ]] || {
    echo "--universal requires macOS and lipo" >&2
    exit 1
  }
  command -v lipo >/dev/null 2>&1 || {
    echo "lipo is required for --universal" >&2
    exit 1
  }

  build_binary_for_target "aarch64-apple-darwin"
  build_binary_for_target "x86_64-apple-darwin"

  UNIVERSAL_DIR="$ROOT_DIR/target/universal-apple-darwin/release"
  mkdir -p "$UNIVERSAL_DIR"
  BINARY_PATH="$UNIVERSAL_DIR/$NAME"
  lipo -create \
    "$ROOT_DIR/target/aarch64-apple-darwin/release/$NAME" \
    "$ROOT_DIR/target/x86_64-apple-darwin/release/$NAME" \
    -output "$BINARY_PATH"

  BUILD_TARGET="universal-apple-darwin"
  BUNDLE_TARGETS=("aarch64-apple-darwin" "x86_64-apple-darwin")
else
  BUILD_TARGET="${TARGET:-$HOST_TARGET}"

  if [[ -n "$TARGET" ]]; then
    build_binary_for_target "$TARGET"
    BINARY_PATH="$ROOT_DIR/target/$TARGET/release/$NAME"
  else
    (
      cd "$ROOT_DIR"
      RUSTFLAGS="$PACKAGE_RUSTFLAGS" cargo build --release --locked
    )
    BINARY_PATH="$ROOT_DIR/target/release/$NAME"
  fi

  BUNDLE_TARGETS=("$BUILD_TARGET")
fi

[[ -x "$BINARY_PATH" ]] || { echo "Built binary not found: $BINARY_PATH" >&2; exit 1; }

BUNDLE_NAME="${NAME}-${VERSION}-${BUILD_TARGET}"
BUNDLE_DIR="$OUTPUT_DIR/$BUNDLE_NAME"
ARCHIVE_PATH="$OUTPUT_DIR/${BUNDLE_NAME}.tar.gz"
CHECKSUM_PATH="${ARCHIVE_PATH}.sha256"
LATEST_ARCHIVE_PATH="$OUTPUT_DIR/${NAME}-${BUILD_TARGET}.tar.gz"
LATEST_CHECKSUM_PATH="${LATEST_ARCHIVE_PATH}.sha256"

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
for root_doc in README.md LICENSE CHANGELOG.md SUPPORT.md SECURITY.md CONTRIBUTING.md ROADMAP.md; do
  if [[ -f "$ROOT_DIR/$root_doc" ]]; then
    install -m 644 "$ROOT_DIR/$root_doc" "$BUNDLE_DIR/$root_doc"
  fi
done
cp -R "$ROOT_DIR/docs/." "$BUNDLE_DIR/docs/"
find "$BUNDLE_DIR/docs" -type d -exec chmod 755 {} \;
find "$BUNDLE_DIR/docs" -type f -exec chmod 644 {} \;
install -m 644 "$FORMULA_TEMPLATE" "$BUNDLE_DIR/packaging/homebrew/${NAME}.rb.template"
printf '%s\n' "$VERSION" > "$BUNDLE_DIR/VERSION"
printf '%s\n' "${BUNDLE_TARGETS[@]}" > "$BUNDLE_DIR/TARGETS"

"$BINARY_PATH" completions bash > "$BUNDLE_DIR/completions/bash/$NAME"
"$BINARY_PATH" completions zsh > "$BUNDLE_DIR/completions/zsh/_$NAME"
"$BINARY_PATH" completions fish > "$BUNDLE_DIR/completions/fish/$NAME.fish"

"$SCRUB_SCRIPT" "$BUNDLE_DIR/bin/$NAME"
if [[ "$(uname -s)" == "Darwin" ]] && command -v codesign >/dev/null 2>&1; then
  codesign --force --sign - "$BUNDLE_DIR/bin/$NAME"
fi

if [[ "$AUDIT" -eq 1 ]]; then
  "$AUDIT_SCRIPT" --bundle "$BUNDLE_DIR"
fi

rm -f "$ARCHIVE_PATH" "$CHECKSUM_PATH" "$LATEST_ARCHIVE_PATH" "$LATEST_CHECKSUM_PATH"
tar -C "$OUTPUT_DIR" -czf "$ARCHIVE_PATH" "$BUNDLE_NAME"
SHA256="$(shasum -a 256 "$ARCHIVE_PATH" | awk '{print $1}')"
printf '%s  %s\n' "$SHA256" "$(basename "$ARCHIVE_PATH")" > "$CHECKSUM_PATH"
cp "$ARCHIVE_PATH" "$LATEST_ARCHIVE_PATH"
printf '%s  %s\n' "$SHA256" "$(basename "$LATEST_ARCHIVE_PATH")" > "$LATEST_CHECKSUM_PATH"

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
  Latest:   $LATEST_ARCHIVE_PATH
  Latest256:$LATEST_CHECKSUM_PATH
  Formula:  $BUNDLE_DIR/packaging/homebrew/${NAME}.rb
  Targets:  ${BUNDLE_TARGETS[*]}
EOF
