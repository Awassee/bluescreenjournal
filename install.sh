#!/usr/bin/env bash
set -euo pipefail

SCRIPT_PATH="${BASH_SOURCE[0]:-}"
if [[ -n "$SCRIPT_PATH" && -e "$SCRIPT_PATH" ]]; then
  ROOT_DIR="$(cd "$(dirname "$SCRIPT_PATH")" && pwd)"
else
  ROOT_DIR=""
fi

MODE="auto"
PREFIX="${BSJ_INSTALL_PREFIX:-}"
BIN_DIR="${BSJ_INSTALL_BIN_DIR:-}"
DOC_DIR="${BSJ_INSTALL_DOC_DIR:-}"
MAN_DIR="${BSJ_INSTALL_MAN_DIR:-}"
BASH_COMPLETION_DIR="${BSJ_INSTALL_BASH_COMPLETION_DIR:-}"
ZSH_COMPLETION_DIR="${BSJ_INSTALL_ZSH_COMPLETION_DIR:-}"
FISH_COMPLETION_DIR="${BSJ_INSTALL_FISH_COMPLETION_DIR:-}"
GITHUB_REPO="${BSJ_INSTALL_REPO:-Awassee/bluescreenjournal}"
RELEASE_VERSION="${BSJ_INSTALL_VERSION:-latest}"
ARCHIVE_SOURCE="${BSJ_INSTALL_ARCHIVE:-}"
SKIP_CHECKSUM=0

if [[ -t 1 ]]; then
  BLUE="$(printf '\033[34m')"
  GREEN="$(printf '\033[32m')"
  YELLOW="$(printf '\033[33m')"
  RED="$(printf '\033[31m')"
  BOLD="$(printf '\033[1m')"
  RESET="$(printf '\033[0m')"
else
  BLUE=""
  GREEN=""
  YELLOW=""
  RED=""
  BOLD=""
  RESET=""
fi

info() {
  printf "%s==>%s %s\n" "$BLUE$BOLD" "$RESET" "$1"
}

warn() {
  printf "%sWarning:%s %s\n" "$YELLOW$BOLD" "$RESET" "$1"
}

die() {
  printf "%sError:%s %s\n" "$RED$BOLD" "$RESET" "$1" >&2
  exit 1
}

usage() {
  cat <<'EOF'
bsj installer

Turnkey install, including the downloader:
  curl -fsSL https://raw.githubusercontent.com/Awassee/bluescreenjournal/main/install.sh | bash

Usage:
  ./install.sh [--source|--prebuilt] [--prefix PATH] [--bin-dir PATH] [--doc-dir PATH] [--man-dir PATH]
               [--bash-completion-dir PATH] [--zsh-completion-dir PATH] [--fish-completion-dir PATH]
               [--repo OWNER/REPO] [--version TAG] [--archive PATH_OR_URL] [--skip-checksum]
  ./install.sh --help

Modes:
  --prebuilt  Install a bundled prebuilt binary. If no local bundle exists, download one from GitHub Releases.
  --source    Build from source. If no local checkout exists, download the source archive first.
  default     Use a local bundle if present, else a local checkout, else download the latest release bundle.

Bootstrap options:
  --repo OWNER/REPO   GitHub repository to download from when bootstrapping
  --version TAG       Release tag to install, defaults to latest
  --archive PATH_OR_URL  Install from a specific .tar.gz bundle instead of GitHub Releases
  --skip-checksum     Skip .sha256 verification for downloaded archives

Install location options:
  --prefix PATH   Install prefix for prebuilt installs or cargo --root for source installs
  --bin-dir PATH  Override the binary install directory for prebuilt installs
  --doc-dir PATH  Override the documentation install directory for prebuilt installs
  --man-dir PATH  Override the man page install directory for prebuilt installs
  --bash-completion-dir PATH  Override the Bash completion install directory
  --zsh-completion-dir PATH   Override the Zsh completion install directory
  --fish-completion-dir PATH  Override the Fish completion install directory

Environment overrides:
  BSJ_INSTALL_PREFIX
  BSJ_INSTALL_BIN_DIR
  BSJ_INSTALL_DOC_DIR
  BSJ_INSTALL_MAN_DIR
  BSJ_INSTALL_BASH_COMPLETION_DIR
  BSJ_INSTALL_ZSH_COMPLETION_DIR
  BSJ_INSTALL_FISH_COMPLETION_DIR
  BSJ_INSTALL_REPO
  BSJ_INSTALL_VERSION
  BSJ_INSTALL_ARCHIVE

Examples:
  curl -fsSL https://raw.githubusercontent.com/Awassee/bluescreenjournal/main/install.sh | bash
  curl -fsSL https://raw.githubusercontent.com/Awassee/bluescreenjournal/main/install.sh | bash -s -- --prefix "$HOME/.local"
  curl -fsSL https://raw.githubusercontent.com/Awassee/bluescreenjournal/main/install.sh | bash -s -- --version v0.1.5
  curl -fsSL https://raw.githubusercontent.com/Awassee/bluescreenjournal/main/install.sh | bash -s -- --source
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --help|-h)
      usage
      exit 0
      ;;
    --source)
      MODE="source"
      shift
      ;;
    --prebuilt)
      MODE="prebuilt"
      shift
      ;;
    --prefix)
      [[ $# -ge 2 ]] || die "--prefix requires a value"
      PREFIX="$2"
      shift 2
      ;;
    --bin-dir)
      [[ $# -ge 2 ]] || die "--bin-dir requires a value"
      BIN_DIR="$2"
      shift 2
      ;;
    --doc-dir)
      [[ $# -ge 2 ]] || die "--doc-dir requires a value"
      DOC_DIR="$2"
      shift 2
      ;;
    --man-dir)
      [[ $# -ge 2 ]] || die "--man-dir requires a value"
      MAN_DIR="$2"
      shift 2
      ;;
    --bash-completion-dir)
      [[ $# -ge 2 ]] || die "--bash-completion-dir requires a value"
      BASH_COMPLETION_DIR="$2"
      shift 2
      ;;
    --zsh-completion-dir)
      [[ $# -ge 2 ]] || die "--zsh-completion-dir requires a value"
      ZSH_COMPLETION_DIR="$2"
      shift 2
      ;;
    --fish-completion-dir)
      [[ $# -ge 2 ]] || die "--fish-completion-dir requires a value"
      FISH_COMPLETION_DIR="$2"
      shift 2
      ;;
    --repo)
      [[ $# -ge 2 ]] || die "--repo requires a value"
      GITHUB_REPO="$2"
      shift 2
      ;;
    --version)
      [[ $# -ge 2 ]] || die "--version requires a value"
      RELEASE_VERSION="$2"
      shift 2
      ;;
    --archive)
      [[ $# -ge 2 ]] || die "--archive requires a value"
      ARCHIVE_SOURCE="$2"
      shift 2
      ;;
    --skip-checksum)
      SKIP_CHECKSUM=1
      shift
      ;;
    *)
      die "Unknown option: $1"
      ;;
  esac
done

[[ "$(uname -s)" == "Darwin" ]] || die "This installer targets macOS."

local_bundle_root() {
  [[ -n "$ROOT_DIR" && -x "$ROOT_DIR/bin/bsj" ]]
}

local_source_root() {
  [[ -n "$ROOT_DIR" && -f "$ROOT_DIR/Cargo.toml" && -d "$ROOT_DIR/src" ]]
}

pick_mode() {
  if [[ "$MODE" != "auto" ]]; then
    printf "%s" "$MODE"
    return
  fi
  if local_bundle_root; then
    printf "prebuilt"
  elif local_source_root; then
    printf "source"
  else
    printf "prebuilt"
  fi
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || die "Required command not found: $1"
}

ensure_path_hint() {
  local path_entry="$1"
  if [[ ":$PATH:" != *":$path_entry:"* ]]; then
    warn "$path_entry is not on PATH."
    printf "Add this line to ~/.zshrc:\n  export PATH=\"%s:\$PATH\"\n" "$path_entry"
  fi
}

common_install_args() {
  local args=()
  if [[ -n "$PREFIX" ]]; then
    args+=(--prefix "$PREFIX")
  fi
  if [[ -n "$BIN_DIR" ]]; then
    args+=(--bin-dir "$BIN_DIR")
  fi
  if [[ -n "$DOC_DIR" ]]; then
    args+=(--doc-dir "$DOC_DIR")
  fi
  if [[ -n "$MAN_DIR" ]]; then
    args+=(--man-dir "$MAN_DIR")
  fi
  if [[ -n "$BASH_COMPLETION_DIR" ]]; then
    args+=(--bash-completion-dir "$BASH_COMPLETION_DIR")
  fi
  if [[ -n "$ZSH_COMPLETION_DIR" ]]; then
    args+=(--zsh-completion-dir "$ZSH_COMPLETION_DIR")
  fi
  if [[ -n "$FISH_COMPLETION_DIR" ]]; then
    args+=(--fish-completion-dir "$FISH_COMPLETION_DIR")
  fi
  printf '%s\n' "${args[@]}"
}

install_completion_files() {
  local source_root="$1"
  local binary_path="$2"
  local prefix="$3"
  local bash_dir="${BASH_COMPLETION_DIR:-$prefix/share/bash-completion/completions}"
  local zsh_dir="${ZSH_COMPLETION_DIR:-$prefix/share/zsh/site-functions}"
  local fish_dir="${FISH_COMPLETION_DIR:-$prefix/share/fish/vendor_completions.d}"

  mkdir -p "$bash_dir" "$zsh_dir" "$fish_dir"

  if [[ -f "$source_root/completions/bash/bsj" ]]; then
    install -m 644 "$source_root/completions/bash/bsj" "$bash_dir/bsj"
  else
    "$binary_path" completions bash > "$bash_dir/bsj"
  fi

  if [[ -f "$source_root/completions/zsh/_bsj" ]]; then
    install -m 644 "$source_root/completions/zsh/_bsj" "$zsh_dir/_bsj"
  else
    "$binary_path" completions zsh > "$zsh_dir/_bsj"
  fi

  if [[ -f "$source_root/completions/fish/bsj.fish" ]]; then
    install -m 644 "$source_root/completions/fish/bsj.fish" "$fish_dir/bsj.fish"
  else
    "$binary_path" completions fish > "$fish_dir/bsj.fish"
  fi

  printf "%sInstalled completions:%s\n" "$GREEN$BOLD" "$RESET"
  printf "  bash  %s/bsj\n" "$bash_dir"
  printf "  zsh   %s/_bsj\n" "$zsh_dir"
  printf "  fish  %s/bsj.fish\n" "$fish_dir"
}

release_asset_name() {
  printf 'bsj-universal-apple-darwin.tar.gz'
}

release_checksum_name() {
  printf '%s.sha256' "$(release_asset_name)"
}

archive_source_name() {
  local source="$1"
  source="${source%%\#*}"
  source="${source%%\?*}"
  basename "$source"
}

release_download_base() {
  if [[ "$RELEASE_VERSION" == "latest" ]]; then
    printf 'https://github.com/%s/releases/latest/download' "$GITHUB_REPO"
  else
    printf 'https://github.com/%s/releases/download/%s' "$GITHUB_REPO" "$RELEASE_VERSION"
  fi
}

release_bundle_url() {
  printf '%s/%s' "$(release_download_base)" "$(release_asset_name)"
}

release_checksum_url() {
  printf '%s/%s' "$(release_download_base)" "$(release_checksum_name)"
}

download_to() {
  local url="$1"
  local output="$2"
  require_command curl
  [[ "$url" =~ ^https:// ]] || die "Refusing insecure download URL: $url"
  info "Downloading $(basename "$output")"
  curl --proto '=https' --tlsv1.2 --fail --location --retry 3 --retry-delay 1 --silent --show-error "$url" --output "$output"
}

copy_or_download_archive() {
  local source="$1"
  local output="$2"
  if [[ "$source" =~ ^https?:// ]]; then
    download_to "$source" "$output"
  else
    [[ -f "$source" ]] || die "Archive not found: $source"
    cp "$source" "$output"
  fi
}

maybe_download_checksum() {
  local checksum_source="$1"
  local output="$2"
  if [[ "$SKIP_CHECKSUM" -eq 1 ]]; then
    return 0
  fi
  if [[ "$checksum_source" =~ ^https?:// ]]; then
    [[ "$checksum_source" =~ ^https:// ]] || die "Refusing insecure checksum URL: $checksum_source"
    curl --proto '=https' --tlsv1.2 --fail --location --retry 3 --retry-delay 1 --silent --show-error "$checksum_source" --output "$output"
    return 0
  elif [[ -f "$checksum_source" ]]; then
    cp "$checksum_source" "$output"
    return 0
  fi
  return 1
}

verify_archive_if_possible() {
  local archive_path="$1"
  local checksum_path="$2"
  local expected checksum_target actual
  if [[ "$SKIP_CHECKSUM" -eq 1 ]]; then
    warn "Skipping checksum verification by request."
    return
  fi
  [[ -f "$checksum_path" ]] || die "Checksum file missing for $(basename "$archive_path")"
  require_command shasum
  info "Verifying archive checksum"
  expected="$(awk '{print $1; exit}' "$checksum_path")"
  [[ -n "$expected" ]] || die "Checksum file is empty: $checksum_path"
  checksum_target="$(awk 'NF >= 2 {print $2; exit}' "$checksum_path")"
  if [[ -n "$checksum_target" ]]; then
    checksum_target="${checksum_target#./}"
    [[ "$checksum_target" == "$(basename "$archive_path")" ]] || die "Checksum file does not match archive name $(basename "$archive_path")"
  fi
  actual="$(shasum -a 256 "$archive_path" | awk '{print $1}')"
  [[ "$actual" == "$expected" ]] || die "Checksum mismatch for $(basename "$archive_path")"
}

extract_archive_bundle() {
  local archive_path="$1"
  local tmp_dir="$2"
  require_command tar
  tar -C "$tmp_dir" -xzf "$archive_path"
  find "$tmp_dir" -mindepth 1 -maxdepth 1 -type d | head -n 1
}

bootstrap_prebuilt_install() {
  local tmp_dir archive_path checksum_path bundle_dir checksum_source require_checksum archive_name
  tmp_dir="$(mktemp -d /tmp/bsj-install.XXXXXX)"
  require_checksum=0

  if [[ -n "$ARCHIVE_SOURCE" ]]; then
    archive_name="$(archive_source_name "$ARCHIVE_SOURCE")"
    archive_path="$tmp_dir/$archive_name"
    checksum_path="${archive_path}.sha256"
    info "Using provided archive source"
    copy_or_download_archive "$ARCHIVE_SOURCE" "$archive_path"
    checksum_source="${ARCHIVE_SOURCE}.sha256"
    if [[ "$ARCHIVE_SOURCE" =~ ^https?:// ]]; then
      require_checksum=1
    fi
  else
    archive_path="$tmp_dir/$(release_asset_name)"
    checksum_path="$tmp_dir/$(release_checksum_name)"
    info "Bootstrapping public release from GitHub"
    download_to "$(release_bundle_url)" "$archive_path"
    checksum_source="$(release_checksum_url)"
    require_checksum=1
  fi

  if ! maybe_download_checksum "$checksum_source" "$checksum_path"; then
    if [[ "$require_checksum" -eq 1 && "$SKIP_CHECKSUM" -eq 0 ]]; then
      die "Checksum download failed for $(basename "$archive_path"). Re-run with --skip-checksum only if you trust the source."
    fi
    warn "No checksum file available; continuing without checksum verification."
  fi

  verify_archive_if_possible "$archive_path" "$checksum_path"

  bundle_dir="$(extract_archive_bundle "$archive_path" "$tmp_dir")"
  [[ -n "$bundle_dir" && -d "$bundle_dir" ]] || die "Extracted bundle directory not found"
  [[ -x "$bundle_dir/install.sh" ]] || die "Bundled installer not found in archive"

  info "Launching bundled installer"
  local delegate_args=()
  while IFS= read -r arg; do
    [[ -n "$arg" ]] && delegate_args+=("$arg")
  done < <(common_install_args)
  "$bundle_dir/install.sh" --prebuilt "${delegate_args[@]}"
}

bootstrap_source_install() {
  require_command curl
  require_command tar

  local tmp_dir source_archive_url source_archive_path source_dir ref_label
  tmp_dir="$(mktemp -d /tmp/bsj-source-install.XXXXXX)"
  source_archive_path="$tmp_dir/source.tar.gz"
  if [[ "$RELEASE_VERSION" == "latest" ]]; then
    ref_label="main"
    source_archive_url="https://github.com/${GITHUB_REPO}/archive/refs/heads/main.tar.gz"
  else
    ref_label="$RELEASE_VERSION"
    source_archive_url="https://github.com/${GITHUB_REPO}/archive/refs/tags/${RELEASE_VERSION}.tar.gz"
  fi

  info "Downloading source archive for $GITHUB_REPO ($ref_label)"
  download_to "$source_archive_url" "$source_archive_path"
  tar -C "$tmp_dir" -xzf "$source_archive_path"
  source_dir="$(find "$tmp_dir" -mindepth 1 -maxdepth 1 -type d | head -n 1)"
  [[ -n "$source_dir" && -f "$source_dir/install.sh" ]] || die "Downloaded source archive is missing install.sh"

  info "Launching source installer"
  local delegate_args=()
  while IFS= read -r arg; do
    [[ -n "$arg" ]] && delegate_args+=("$arg")
  done < <(common_install_args)
  "$source_dir/install.sh" --source "${delegate_args[@]}"
}

install_prebuilt() {
  if ! local_bundle_root; then
    bootstrap_prebuilt_install
    return
  fi

  local prefix="${PREFIX:-$HOME/.local}"
  local final_bin_dir="${BIN_DIR:-$prefix/bin}"
  local final_doc_dir="${DOC_DIR:-$prefix/share/doc/bsj}"
  local final_man_dir="${MAN_DIR:-$prefix/share/man/man1}"
  local final_example_dir="$prefix/share/bsj/examples"

  info "Installing bundled bsj into $final_bin_dir"
  mkdir -p "$final_bin_dir" "$final_doc_dir" "$final_man_dir" "$final_example_dir"

  install -m 755 "$ROOT_DIR/bin/bsj" "$final_bin_dir/bsj"

  for root_doc in README.md LICENSE CHANGELOG.md SUPPORT.md SECURITY.md CONTRIBUTING.md ROADMAP.md; do
    if [[ -f "$ROOT_DIR/$root_doc" ]]; then
      install -m 644 "$ROOT_DIR/$root_doc" "$final_doc_dir/$root_doc"
    fi
  done
  if [[ -d "$ROOT_DIR/docs" ]]; then
    mkdir -p "$final_doc_dir/docs"
    cp -R "$ROOT_DIR/docs/." "$final_doc_dir/docs/"
    find "$final_doc_dir/docs" -type d -exec chmod 755 {} \;
    find "$final_doc_dir/docs" -type f -exec chmod 644 {} \;
    if [[ -f "$ROOT_DIR/docs/config.example.json" ]]; then
      install -m 644 "$ROOT_DIR/docs/config.example.json" "$final_example_dir/config.example.json"
    fi
    if [[ -f "$ROOT_DIR/docs/bsj.1" ]]; then
      install -m 644 "$ROOT_DIR/docs/bsj.1" "$final_man_dir/bsj.1"
    fi
  fi

  install_completion_files "$ROOT_DIR" "$final_bin_dir/bsj" "$prefix"

  printf "%sInstalled binary:%s %s\n" "$GREEN$BOLD" "$RESET" "$final_bin_dir/bsj"
  printf "%sInstalled docs:%s %s\n" "$GREEN$BOLD" "$RESET" "$final_doc_dir"
  printf "%sInstalled examples:%s %s\n" "$GREEN$BOLD" "$RESET" "$final_example_dir"
  printf "%sInstalled man page:%s %s\n" "$GREEN$BOLD" "$RESET" "$final_man_dir/bsj.1"

  ensure_path_hint "$final_bin_dir"

  cat <<EOF

Next steps:
  1. $final_bin_dir/bsj guide setup
  2. $final_bin_dir/bsj
  3. $final_bin_dir/bsj settings
  4. man $final_man_dir/bsj.1
EOF
}

ensure_xcode_cli_tools() {
  if xcode-select -p >/dev/null 2>&1; then
    return
  fi
  warn "Xcode Command Line Tools are required to build bsj from source."
  xcode-select --install >/dev/null 2>&1 || true
  die "The Xcode Command Line Tools installer has been launched. Re-run this command after it finishes, or use the default prebuilt install path instead."
}

ensure_rust_toolchain() {
  if command -v cargo >/dev/null 2>&1 && command -v rustc >/dev/null 2>&1; then
    return
  fi
  require_command curl
  info "Installing Rust toolchain with rustup"
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
  export PATH="$HOME/.cargo/bin:$PATH"
  command -v cargo >/dev/null 2>&1 || die "Rust install completed, but cargo is still not on PATH. Add ~/.cargo/bin to PATH and retry."
}

install_from_source() {
  if ! local_source_root; then
    bootstrap_source_install
    return
  fi

  [[ -z "$DOC_DIR" ]] || die "--doc-dir is only supported for prebuilt installs"
  [[ -z "$MAN_DIR" ]] || die "--man-dir is only supported for prebuilt installs"

  ensure_xcode_cli_tools
  ensure_rust_toolchain

  local cargo_root="${PREFIX:-${CARGO_HOME:-$HOME/.cargo}}"
  if [[ -n "$BIN_DIR" && "$BIN_DIR" != "$cargo_root/bin" ]]; then
    die "--bin-dir is only supported for prebuilt installs; use --prefix for source installs"
  fi
  local cargo_bin_dir="${BIN_DIR:-$cargo_root/bin}"
  local config_dir="$HOME/Library/Application Support/bsj"
  info "Installing bsj from source into $cargo_bin_dir"
  cargo install --path "$ROOT_DIR" --locked --force --root "$cargo_root"
  mkdir -p "$config_dir"

  local bsj_bin="$cargo_bin_dir/bsj"
  [[ -x "$bsj_bin" ]] || die "Install finished but $bsj_bin was not created."

  printf "%sInstalled binary:%s %s\n" "$GREEN$BOLD" "$RESET" "$bsj_bin"
  printf "%sConfig dir:%s %s\n" "$GREEN$BOLD" "$RESET" "$config_dir"
  install_completion_files "$ROOT_DIR" "$bsj_bin" "$cargo_root"

  ensure_path_hint "$cargo_bin_dir"

  cat <<EOF

Next steps:
  1. $bsj_bin guide setup
  2. $bsj_bin
  3. $bsj_bin settings
EOF
}

case "$(pick_mode)" in
  prebuilt)
    install_prebuilt
    ;;
  source)
    install_from_source
    ;;
  *)
    die "Unsupported installer mode"
    ;;
esac
