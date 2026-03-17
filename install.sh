#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
MODE="auto"
PREFIX="${BSJ_INSTALL_PREFIX:-}"
BIN_DIR="${BSJ_INSTALL_BIN_DIR:-}"
DOC_DIR="${BSJ_INSTALL_DOC_DIR:-}"
MAN_DIR="${BSJ_INSTALL_MAN_DIR:-}"
BASH_COMPLETION_DIR="${BSJ_INSTALL_BASH_COMPLETION_DIR:-}"
ZSH_COMPLETION_DIR="${BSJ_INSTALL_ZSH_COMPLETION_DIR:-}"
FISH_COMPLETION_DIR="${BSJ_INSTALL_FISH_COMPLETION_DIR:-}"

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

Usage:
  ./install.sh [--source|--prebuilt] [--prefix PATH] [--bin-dir PATH] [--doc-dir PATH] [--man-dir PATH]
               [--bash-completion-dir PATH] [--zsh-completion-dir PATH] [--fish-completion-dir PATH]
  ./install.sh --help

Modes:
  --source    Build and install from the current source checkout with Cargo
  --prebuilt  Install a bundled prebuilt binary from ./bin/bsj
  default     Auto-detect prebuilt bundle first, then fall back to source

Options:
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

What it does:
  - source mode checks for macOS, Xcode Command Line Tools, Rust, and Cargo
  - prebuilt mode copies the bundled binary, docs, example config, man page, and shell completions
  - both modes print next-step commands after install
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
    *)
      die "Unknown option: $1"
      ;;
  esac
done

[[ "$(uname -s)" == "Darwin" ]] || die "This installer targets macOS."

has_prebuilt_bundle() {
  [[ -x "$ROOT_DIR/bin/bsj" ]]
}

pick_mode() {
  if [[ "$MODE" != "auto" ]]; then
    printf "%s" "$MODE"
    return
  fi
  if has_prebuilt_bundle; then
    printf "prebuilt"
  else
    printf "source"
  fi
}

ensure_path_hint() {
  local path_entry="$1"
  if [[ ":$PATH:" != *":$path_entry:"* ]]; then
    warn "$path_entry is not on PATH."
    printf "Add this line to ~/.zshrc:\n  export PATH=\"%s:\$PATH\"\n" "$path_entry"
  fi
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

install_prebuilt() {
  has_prebuilt_bundle || die "Prebuilt bundle not found. Expected $ROOT_DIR/bin/bsj"

  local prefix="${PREFIX:-$HOME/.local}"
  local final_bin_dir="${BIN_DIR:-$prefix/bin}"
  local final_doc_dir="${DOC_DIR:-$prefix/share/doc/bsj}"
  local final_man_dir="${MAN_DIR:-$prefix/share/man/man1}"
  local final_example_dir="$prefix/share/bsj/examples"

  info "Installing bundled bsj into $final_bin_dir"
  mkdir -p "$final_bin_dir" "$final_doc_dir" "$final_man_dir" "$final_example_dir"

  install -m 755 "$ROOT_DIR/bin/bsj" "$final_bin_dir/bsj"

  if [[ -f "$ROOT_DIR/README.md" ]]; then
    install -m 644 "$ROOT_DIR/README.md" "$final_doc_dir/README.md"
  fi
  if [[ -f "$ROOT_DIR/LICENSE" ]]; then
    install -m 644 "$ROOT_DIR/LICENSE" "$final_doc_dir/LICENSE"
  fi
  if [[ -d "$ROOT_DIR/docs" ]]; then
    find "$ROOT_DIR/docs" -maxdepth 1 -type f -name '*.md' ! -name 'bsj.1' -exec install -m 644 {} "$final_doc_dir" \;
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

install_from_source() {
  command -v cargo >/dev/null 2>&1 || die "Cargo was not found. Install Rust first: https://rustup.rs/"
  command -v rustc >/dev/null 2>&1 || die "rustc was not found. Install Rust first: https://rustup.rs/"

  [[ -z "$DOC_DIR" ]] || die "--doc-dir is only supported for prebuilt installs"
  [[ -z "$MAN_DIR" ]] || die "--man-dir is only supported for prebuilt installs"

  if ! xcode-select -p >/dev/null 2>&1; then
    die "Xcode Command Line Tools are required. Run: xcode-select --install"
  fi

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
