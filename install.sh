#!/usr/bin/env bash
set -euo pipefail

SCRIPT_PATH="${BASH_SOURCE[0]:-}"
if [[ -n "$SCRIPT_PATH" && -e "$SCRIPT_PATH" ]]; then
  ROOT_DIR="$(cd "$(dirname "$SCRIPT_PATH")" && pwd)"
else
  ROOT_DIR=""
fi

MODE="auto"
ACTION="install"
ACTION_EXPLICIT=0
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
ASSUME_YES=0
ORIGINAL_ARG_COUNT=$#
declare -a UNINSTALL_TARGETS
declare -a DATA_TARGETS
declare -a KEYCHAIN_VAULT_PATHS
declare -a CONFIG_TARGETS

PRODUCT_NAME="BlueScreen Journal"
PRODUCT_COPYRIGHT="(c) 2026 Awassee LLC and Sean Heiney"
PRODUCT_CONTACT="sean@sean.net"
PRODUCT_REPO_URL="https://github.com/Awassee/bluescreenjournal"
BANNER_PRINTED=0

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

print_installer_banner() {
  if [[ "$BANNER_PRINTED" -eq 1 ]]; then
    return
  fi
  local line
  line="=============================================================="
  printf "\n%s%s%s\n" "$BLUE$BOLD" "$line" "$RESET"
  printf "%s%s Installer%s\n" "$BLUE$BOLD" "$PRODUCT_NAME" "$RESET"
  printf "%sNostalgia-first encrypted journal setup for macOS%s\n" "$BLUE" "$RESET"
  printf "%s%s%s\n" "$BLUE$BOLD" "$line" "$RESET"
  BANNER_PRINTED=1
}

print_about() {
  cat <<EOF
$PRODUCT_NAME Installer

$PRODUCT_COPYRIGHT
Contact: $PRODUCT_CONTACT
Repository: $PRODUCT_REPO_URL

This installer can:
  - install/update stable prebuilt bundles
  - build latest source from main
  - run troubleshooting diagnostics
  - repair PATH integration
  - uninstall or factory reset
EOF
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
               [--uninstall|--factory-reset|--doctor|--repair-path|--about] [--yes]
  ./install.sh --help

Modes:
  --prebuilt  Install a bundled prebuilt binary. If no local bundle exists, download one from GitHub Releases.
  --source    Build from source. If no local checkout exists, download the source archive first.
  default     Use a local bundle if present, else a local checkout, else:
              - if bsj is already installed: build latest main from source (update flow)
              - otherwise: install latest prebuilt GitHub release

Bootstrap options:
  --repo OWNER/REPO   GitHub repository to download from when bootstrapping
  --version TAG       Release tag to install, defaults to latest
  --archive PATH_OR_URL  Install from a specific .tar.gz bundle instead of GitHub Releases
  --skip-checksum     Skip .sha256 verification for downloaded archives
  --yes, -y           Skip interactive confirmations for uninstall/factory-reset

Reset options:
  --uninstall         Remove bsj binaries/docs/completions. Keeps journal vault data.
  --factory-reset     Remove bsj install plus config/logs/keychain and local vault data.
  --doctor            Run installer diagnostics and troubleshooting checks.
  --repair-path       Add a bsj binary directory to shell startup PATH files.
  --about             Print installer about/copyright info.

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
  curl -fsSL https://raw.githubusercontent.com/Awassee/bluescreenjournal/main/install.sh | bash -s -- --version v1.2.1
  curl -fsSL https://raw.githubusercontent.com/Awassee/bluescreenjournal/main/install.sh | bash -s -- --source
  curl -fsSL https://raw.githubusercontent.com/Awassee/bluescreenjournal/main/install.sh | bash -s -- --doctor
  curl -fsSL https://raw.githubusercontent.com/Awassee/bluescreenjournal/main/install.sh | bash -s -- --repair-path
  ./install.sh --uninstall
  ./install.sh --factory-reset
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
    --uninstall)
      [[ "$ACTION" == "install" ]] || die "Choose either --uninstall or --factory-reset, not both."
      ACTION="uninstall"
      ACTION_EXPLICIT=1
      shift
      ;;
    --factory-reset)
      [[ "$ACTION" == "install" ]] || die "Choose either --uninstall or --factory-reset, not both."
      ACTION="factory_reset"
      ACTION_EXPLICIT=1
      shift
      ;;
    --doctor)
      [[ "$ACTION" == "install" ]] || die "Choose one action at a time (--doctor cannot be combined with uninstall/reset)."
      ACTION="doctor"
      ACTION_EXPLICIT=1
      shift
      ;;
    --repair-path)
      [[ "$ACTION" == "install" ]] || die "Choose one action at a time (--repair-path cannot be combined with uninstall/reset)."
      ACTION="path_repair"
      ACTION_EXPLICIT=1
      shift
      ;;
    --about)
      print_about
      exit 0
      ;;
    --yes|-y)
      ASSUME_YES=1
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
  elif [[ -z "$ARCHIVE_SOURCE" && "$RELEASE_VERSION" == "latest" ]] && command -v bsj >/dev/null 2>&1; then
    # Update flow: if bsj is already installed, prefer latest main source so reruns actually pick up new commits.
    printf "source"
  else
    printf "prebuilt"
  fi
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || die "Required command not found: $1"
}

make_temp_dir() {
  local label="$1"
  local temp_root="${TMPDIR:-/tmp}"
  mkdir -p "$temp_root"
  mktemp -d "$temp_root/${label}.XXXXXX"
}

tty_input_available() {
  if ! [[ -t 0 || -t 1 || -t 2 ]]; then
    return 1
  fi
  if ! [[ -r /dev/tty && -w /dev/tty ]]; then
    return 1
  fi
  if ! (: > /dev/tty) >/dev/null 2>&1; then
    return 1
  fi
  return 0
}

read_line_interactive() {
  local prompt="$1"
  if tty_input_available; then
    printf "%s" "$prompt" > /dev/tty || return 1
    IFS= read -r REPLY < /dev/tty || return 1
    return 0
  fi
  if [[ -t 0 ]]; then
    printf "%s" "$prompt"
    IFS= read -r REPLY || return 1
    return 0
  fi
  return 1
}

confirm_yes_no() {
  local prompt="$1"
  local normalized
  if [[ "$ASSUME_YES" -eq 1 ]]; then
    return 0
  fi
  read_line_interactive "$prompt" || die "Cannot prompt for confirmation in non-interactive mode. Re-run with --yes."
  normalized="$(printf "%s" "$REPLY" | tr '[:upper:]' '[:lower:]')"
  [[ "$normalized" == "y" || "$normalized" == "yes" ]]
}

confirm_phrase() {
  local prompt="$1"
  local expected="$2"
  if [[ "$ASSUME_YES" -eq 1 ]]; then
    return 0
  fi
  read_line_interactive "$prompt" || die "Cannot prompt for confirmation in non-interactive mode. Re-run with --yes."
  [[ "$REPLY" == "$expected" ]]
}

maybe_prompt_action_menu() {
  local selection normalized
  if [[ "$ACTION_EXPLICIT" -eq 1 || "$ORIGINAL_ARG_COUNT" -ne 0 ]]; then
    return
  fi
  if ! tty_input_available; then
    return
  fi

  print_installer_banner
  cat <<EOF
$PRODUCT_COPYRIGHT
Contact: $PRODUCT_CONTACT

Choose what to do:
  1) Install / Update (Recommended smart mode)
  2) Install stable prebuilt release
  3) Update from latest main source
  4) Troubleshoot (doctor diagnostics)
  5) Repair PATH integration
  6) Uninstall app files (keep journal data)
  7) Factory reset (remove app + settings + local journal data)
  a) About installer
  h) Help/options
  q) Quit
EOF

  while true; do
    read_line_interactive "Select an option [1-7,a,h,q]: " || die "Failed to read installer menu selection."
    selection="$REPLY"
    normalized="$(printf "%s" "$selection" | tr '[:upper:]' '[:lower:]')"
    case "$normalized" in
      ""|1)
        ACTION="install"
        return
        ;;
      2)
        ACTION="install"
        MODE="prebuilt"
        ACTION_EXPLICIT=1
        return
        ;;
      3)
        ACTION="install"
        MODE="source"
        RELEASE_VERSION="latest"
        ACTION_EXPLICIT=1
        return
        ;;
      4)
        ACTION="doctor"
        ACTION_EXPLICIT=1
        return
        ;;
      5)
        ACTION="path_repair"
        ACTION_EXPLICIT=1
        return
        ;;
      6)
        ACTION="uninstall"
        ACTION_EXPLICIT=1
        return
        ;;
      7)
        ACTION="factory_reset"
        ACTION_EXPLICIT=1
        return
        ;;
      a|about)
        print_about
        ;;
      h|help)
        usage
        ;;
      q|quit|exit)
        info "Installer canceled."
        exit 0
        ;;
      *)
        warn "Please choose 1-7, a, h, or q."
        ;;
    esac
  done
}

expand_home_path() {
  local path_value="$1"
  if [[ "$path_value" == "~" ]]; then
    printf "%s" "$HOME"
  elif [[ "$path_value" == "~/"* ]]; then
    printf "%s/%s" "$HOME" "${path_value:2}"
  else
    printf "%s" "$path_value"
  fi
}

effective_prebuilt_prefix() {
  local active_bsj active_dir
  if [[ -n "$PREFIX" ]]; then
    expand_home_path "$PREFIX"
    return
  fi

  if command -v bsj >/dev/null 2>&1; then
    active_bsj="$(command -v bsj)"
    active_dir="$(dirname "$active_bsj")"
    if [[ -d "$active_dir" && -w "$active_dir" ]]; then
      dirname "$active_dir"
      return
    fi
  fi

  printf "%s" "$HOME/.local"
}

is_dangerous_delete_target() {
  local target="$1"
  local normalized="${target%/}"
  [[ -n "$normalized" ]] || return 0
  case "$normalized" in
    "/"|"/Users"|"/Users/$USER"|"$HOME"|".")
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

json_value_from_file() {
  local file_path="$1"
  local key="$2"
  sed -nE "s/.*\"${key}\"[[:space:]]*:[[:space:]]*\"([^\"]+)\".*/\1/p" "$file_path" | head -n 1
}

json_unescape() {
  local value="$1"
  printf "%s" "$value" | sed 's#\\/#/#g; s#\\\\#\\#g'
}

add_unique_path_entry() {
  local value="$1"
  shift
  local existing
  for existing in "$@"; do
    [[ "$existing" == "$value" ]] && return 1
  done
  return 0
}

collect_uninstall_targets() {
  UNINSTALL_TARGETS=()
  local prefix resolved_path
  local -a prefix_candidates
  prefix_candidates=()

  if [[ -n "$PREFIX" ]]; then
    prefix_candidates+=("$(expand_home_path "$PREFIX")")
  fi
  prefix_candidates+=("$(effective_prebuilt_prefix)")
  prefix_candidates+=("$HOME/.local")
  prefix_candidates+=("${CARGO_HOME:-$HOME/.cargo}")

  for prefix in "${prefix_candidates[@]}"; do
    if add_unique_path_entry "$prefix/bin/bsj" "${UNINSTALL_TARGETS[@]-}"; then
      UNINSTALL_TARGETS+=("$prefix/bin/bsj")
    fi
    if add_unique_path_entry "$prefix/share/doc/bsj" "${UNINSTALL_TARGETS[@]-}"; then
      UNINSTALL_TARGETS+=("$prefix/share/doc/bsj")
    fi
    if add_unique_path_entry "$prefix/share/man/man1/bsj.1" "${UNINSTALL_TARGETS[@]-}"; then
      UNINSTALL_TARGETS+=("$prefix/share/man/man1/bsj.1")
    fi
    if add_unique_path_entry "$prefix/share/bsj" "${UNINSTALL_TARGETS[@]-}"; then
      UNINSTALL_TARGETS+=("$prefix/share/bsj")
    fi
    if add_unique_path_entry "$prefix/share/bash-completion/completions/bsj" "${UNINSTALL_TARGETS[@]-}"; then
      UNINSTALL_TARGETS+=("$prefix/share/bash-completion/completions/bsj")
    fi
    if add_unique_path_entry "$prefix/share/zsh/site-functions/_bsj" "${UNINSTALL_TARGETS[@]-}"; then
      UNINSTALL_TARGETS+=("$prefix/share/zsh/site-functions/_bsj")
    fi
    if add_unique_path_entry "$prefix/share/fish/vendor_completions.d/bsj.fish" "${UNINSTALL_TARGETS[@]-}"; then
      UNINSTALL_TARGETS+=("$prefix/share/fish/vendor_completions.d/bsj.fish")
    fi
  done

  if [[ -n "$BIN_DIR" ]]; then
    resolved_path="$(expand_home_path "$BIN_DIR")/bsj"
    if add_unique_path_entry "$resolved_path" "${UNINSTALL_TARGETS[@]-}"; then
      UNINSTALL_TARGETS+=("$resolved_path")
    fi
  fi
  if [[ -n "$DOC_DIR" ]]; then
    resolved_path="$(expand_home_path "$DOC_DIR")"
    if [[ "$(basename "$resolved_path")" == "bsj" ]]; then
      if add_unique_path_entry "$resolved_path" "${UNINSTALL_TARGETS[@]-}"; then
        UNINSTALL_TARGETS+=("$resolved_path")
      fi
    else
      for doc_name in README.md LICENSE CHANGELOG.md SUPPORT.md SECURITY.md CONTRIBUTING.md ROADMAP.md; do
        if add_unique_path_entry "$resolved_path/$doc_name" "${UNINSTALL_TARGETS[@]-}"; then
          UNINSTALL_TARGETS+=("$resolved_path/$doc_name")
        fi
      done
      if add_unique_path_entry "$resolved_path/docs" "${UNINSTALL_TARGETS[@]-}"; then
        UNINSTALL_TARGETS+=("$resolved_path/docs")
      fi
    fi
  fi
  if [[ -n "$MAN_DIR" ]]; then
    resolved_path="$(expand_home_path "$MAN_DIR")/bsj.1"
    if add_unique_path_entry "$resolved_path" "${UNINSTALL_TARGETS[@]-}"; then
      UNINSTALL_TARGETS+=("$resolved_path")
    fi
  fi
  if [[ -n "$BASH_COMPLETION_DIR" ]]; then
    resolved_path="$(expand_home_path "$BASH_COMPLETION_DIR")/bsj"
    if add_unique_path_entry "$resolved_path" "${UNINSTALL_TARGETS[@]-}"; then
      UNINSTALL_TARGETS+=("$resolved_path")
    fi
  fi
  if [[ -n "$ZSH_COMPLETION_DIR" ]]; then
    resolved_path="$(expand_home_path "$ZSH_COMPLETION_DIR")/_bsj"
    if add_unique_path_entry "$resolved_path" "${UNINSTALL_TARGETS[@]-}"; then
      UNINSTALL_TARGETS+=("$resolved_path")
    fi
  fi
  if [[ -n "$FISH_COMPLETION_DIR" ]]; then
    resolved_path="$(expand_home_path "$FISH_COMPLETION_DIR")/bsj.fish"
    if add_unique_path_entry "$resolved_path" "${UNINSTALL_TARGETS[@]-}"; then
      UNINSTALL_TARGETS+=("$resolved_path")
    fi
  fi

  if command -v bsj >/dev/null 2>&1; then
    resolved_path="$(command -v bsj)"
    if [[ -n "$resolved_path" ]] && add_unique_path_entry "$resolved_path" "${UNINSTALL_TARGETS[@]-}"; then
      UNINSTALL_TARGETS+=("$resolved_path")
    fi
  fi
}

collect_data_targets() {
  DATA_TARGETS=()
  KEYCHAIN_VAULT_PATHS=()
  CONFIG_TARGETS=()

  local config_file resolved_path raw_value vault_value sync_value
  local -a config_candidates
  config_candidates=(
    "$HOME/Library/Application Support/bsj/config.json"
    "${XDG_CONFIG_HOME:-$HOME/.config}/bsj/config.json"
  )

  if add_unique_path_entry "$HOME/Documents/BlueScreenJournal" "${DATA_TARGETS[@]-}"; then
    DATA_TARGETS+=("$HOME/Documents/BlueScreenJournal")
  fi
  if add_unique_path_entry "$HOME/Documents/BlueScreenJournal" "${KEYCHAIN_VAULT_PATHS[@]-}"; then
    KEYCHAIN_VAULT_PATHS+=("$HOME/Documents/BlueScreenJournal")
  fi

  for config_file in "${config_candidates[@]}"; do
    if [[ ! -f "$config_file" ]]; then
      continue
    fi
    if add_unique_path_entry "$(dirname "$config_file")" "${CONFIG_TARGETS[@]-}"; then
      CONFIG_TARGETS+=("$(dirname "$config_file")")
    fi

    raw_value="$(json_value_from_file "$config_file" "vault_path" || true)"
    if [[ -n "$raw_value" ]]; then
      vault_value="$(json_unescape "$raw_value")"
      resolved_path="$(expand_home_path "$vault_value")"
      if add_unique_path_entry "$resolved_path" "${DATA_TARGETS[@]-}"; then
        DATA_TARGETS+=("$resolved_path")
      fi
      if add_unique_path_entry "$resolved_path" "${KEYCHAIN_VAULT_PATHS[@]-}"; then
        KEYCHAIN_VAULT_PATHS+=("$resolved_path")
      fi
    fi

    raw_value="$(json_value_from_file "$config_file" "sync_target_path" || true)"
    if [[ -n "$raw_value" ]]; then
      sync_value="$(json_unescape "$raw_value")"
      resolved_path="$(expand_home_path "$sync_value")"
      if add_unique_path_entry "$resolved_path" "${DATA_TARGETS[@]-}"; then
        DATA_TARGETS+=("$resolved_path")
      fi
    fi
  done

  if add_unique_path_entry "$HOME/Library/Application Support/bsj" "${CONFIG_TARGETS[@]-}"; then
    CONFIG_TARGETS+=("$HOME/Library/Application Support/bsj")
  fi
  if add_unique_path_entry "${XDG_CONFIG_HOME:-$HOME/.config}/bsj" "${CONFIG_TARGETS[@]-}"; then
    CONFIG_TARGETS+=("${XDG_CONFIG_HOME:-$HOME/.config}/bsj")
  fi
  if add_unique_path_entry "$HOME/Library/Logs/bsj" "${CONFIG_TARGETS[@]-}"; then
    CONFIG_TARGETS+=("$HOME/Library/Logs/bsj")
  fi
}

remove_path_if_exists() {
  local target="$1"
  if [[ -e "$target" || -L "$target" ]]; then
    rm -rf "$target"
    info "Removed: $target"
    REMOVED_COUNT=$((REMOVED_COUNT + 1))
  fi
}

remove_path_safely_if_exists() {
  local target="$1"
  if is_dangerous_delete_target "$target"; then
    warn "Skipping unsafe delete target: $target"
    return
  fi
  remove_path_if_exists "$target"
}

remove_keychain_passphrase_for_vault() {
  local vault_path="$1"
  local digest account

  command -v security >/dev/null 2>&1 || return
  command -v shasum >/dev/null 2>&1 || return
  digest="$(printf "%s" "$vault_path" | shasum -a 256 | awk '{print $1}')"
  [[ -n "$digest" ]] || return
  account="vault-${digest:0:16}"

  if security delete-generic-password -a "$account" -s "com.awassee.bsj.passphrase" >/dev/null 2>&1; then
    info "Removed Keychain item for vault path: $vault_path"
  fi
}

run_uninstall() {
  local target
  collect_uninstall_targets

  cat <<EOF

Uninstall mode (keep journal data):
  - removes bsj binaries/docs/completions/man files
  - keeps vault data under ~/Documents/BlueScreenJournal (or your custom vault path)
EOF

  if ! confirm_yes_no "Proceed with uninstall? [y/N]: "; then
    info "Canceled uninstall."
    exit 0
  fi

  REMOVED_COUNT=0
  for target in "${UNINSTALL_TARGETS[@]-}"; do
    remove_path_if_exists "$target"
  done

  printf "%sUninstall complete.%s Removed %s path(s).\n" "$GREEN$BOLD" "$RESET" "$REMOVED_COUNT"
  cat <<'EOF'
Data was preserved.
To fully wipe data for a fresh setup, run:
  ./install.sh --factory-reset
EOF
}

run_factory_reset() {
  local target
  collect_uninstall_targets
  collect_data_targets

  cat <<EOF

Factory reset mode:
  - removes bsj binaries/docs/completions/man files
  - removes bsj config and logs
  - removes local vault data paths and local sync target path from config (if set)
  - removes saved passphrase entries from macOS Keychain for discovered vault paths
EOF

  printf "Data paths to remove:\n"
  for target in "${DATA_TARGETS[@]-}"; do
    printf "  - %s\n" "$target"
  done
  printf "Config/log paths to remove:\n"
  for target in "${CONFIG_TARGETS[@]-}"; do
    printf "  - %s\n" "$target"
  done

  if ! confirm_phrase "Type ERASE to continue: " "ERASE"; then
    info "Canceled factory reset."
    exit 0
  fi

  REMOVED_COUNT=0
  for target in "${UNINSTALL_TARGETS[@]-}"; do
    remove_path_if_exists "$target"
  done
  for target in "${CONFIG_TARGETS[@]-}"; do
    remove_path_safely_if_exists "$target"
  done
  for target in "${DATA_TARGETS[@]-}"; do
    remove_path_safely_if_exists "$target"
  done
  for target in "${KEYCHAIN_VAULT_PATHS[@]-}"; do
    remove_keychain_passphrase_for_vault "$target"
  done

  printf "%sFactory reset complete.%s Removed %s path(s).\n" "$GREEN$BOLD" "$RESET" "$REMOVED_COUNT"
  cat <<'EOF'
You can now run the installer again for a clean first-run setup.
EOF
}

doctor_row() {
  local level="$1"
  local label="$2"
  local detail="$3"
  printf "  %-8s %-24s %s\n" "$level" "$label" "$detail"
}

run_doctor() {
  local failures warnings
  local os_name arch_name shell_name mode_selected
  local release_api_url release_asset
  local tmp_root tmp_free_kb tmp_free_mb
  local install_prefix install_bin_dir
  local active_bsj candidate
  failures=0
  warnings=0

  print_installer_banner
  printf "\nInstaller Doctor\n"
  printf "Checks your environment and surfaces likely install/update blockers.\n\n"

  os_name="$(uname -s 2>/dev/null || printf "unknown")"
  arch_name="$(uname -m 2>/dev/null || printf "unknown")"
  if [[ "$os_name" == "Darwin" ]]; then
    doctor_row "[OK]" "Platform" "$os_name ($arch_name)"
  else
    doctor_row "[FAIL]" "Platform" "Expected Darwin, got $os_name"
    failures=$((failures + 1))
  fi

  shell_name="$(basename "${SHELL:-unknown}")"
  doctor_row "[INFO]" "Shell" "$shell_name"

  for candidate in curl tar shasum; do
    if command -v "$candidate" >/dev/null 2>&1; then
      doctor_row "[OK]" "Command $candidate" "$(command -v "$candidate")"
    else
      doctor_row "[FAIL]" "Command $candidate" "missing"
      failures=$((failures + 1))
    fi
  done

  for candidate in security xcode-select cargo rustc; do
    if command -v "$candidate" >/dev/null 2>&1; then
      doctor_row "[OK]" "Optional $candidate" "$(command -v "$candidate")"
    else
      doctor_row "[WARN]" "Optional $candidate" "not found"
      warnings=$((warnings + 1))
    fi
  done

  mode_selected="$(pick_mode)"
  if [[ "$mode_selected" == "prebuilt" ]]; then
    install_prefix="$(effective_prebuilt_prefix)"
    install_bin_dir="${BIN_DIR:-$install_prefix/bin}"
  else
    install_prefix="$(expand_home_path "${PREFIX:-${CARGO_HOME:-$HOME/.cargo}}")"
    install_bin_dir="${BIN_DIR:-$install_prefix/bin}"
  fi

  if mkdir -p "$install_bin_dir" >/dev/null 2>&1; then
    doctor_row "[OK]" "Install bin dir" "$install_bin_dir"
  else
    doctor_row "[FAIL]" "Install bin dir" "not writable: $install_bin_dir"
    failures=$((failures + 1))
  fi

  tmp_root="${TMPDIR:-/tmp}"
  tmp_free_kb="$(df -Pk "$tmp_root" 2>/dev/null | awk 'NR==2 {print $4}' || true)"
  if [[ "$tmp_free_kb" =~ ^[0-9]+$ ]]; then
    tmp_free_mb=$((tmp_free_kb / 1024))
    if [[ "$tmp_free_kb" -lt 524288 ]]; then
      doctor_row "[FAIL]" "Free space ($tmp_root)" "${tmp_free_mb}MB (<512MB)"
      failures=$((failures + 1))
    elif [[ "$tmp_free_kb" -lt 1048576 ]]; then
      doctor_row "[WARN]" "Free space ($tmp_root)" "${tmp_free_mb}MB (<1024MB)"
      warnings=$((warnings + 1))
    else
      doctor_row "[OK]" "Free space ($tmp_root)" "${tmp_free_mb}MB"
    fi
  else
    doctor_row "[WARN]" "Free space ($tmp_root)" "unknown"
    warnings=$((warnings + 1))
  fi

  if command -v bsj >/dev/null 2>&1; then
    active_bsj="$(command -v bsj)"
    doctor_row "[OK]" "Active bsj on PATH" "$active_bsj"
  else
    doctor_row "[WARN]" "Active bsj on PATH" "not found"
    warnings=$((warnings + 1))
  fi

  release_api_url="https://api.github.com/repos/${GITHUB_REPO}/releases/latest"
  if command -v curl >/dev/null 2>&1; then
    if curl --proto '=https' --tlsv1.2 --fail --location --silent --show-error --max-time 12 --head "$release_api_url" >/dev/null 2>&1; then
      doctor_row "[OK]" "GitHub API reachability" "$release_api_url"
    else
      doctor_row "[WARN]" "GitHub API reachability" "cannot reach $release_api_url"
      warnings=$((warnings + 1))
    fi
  fi

  release_asset="$(release_bundle_url)"
  if command -v curl >/dev/null 2>&1; then
    if curl --proto '=https' --tlsv1.2 --fail --location --silent --show-error --max-time 12 --head "$release_asset" >/dev/null 2>&1; then
      doctor_row "[OK]" "Release asset URL" "$release_asset"
    else
      doctor_row "[WARN]" "Release asset URL" "cannot reach $release_asset"
      warnings=$((warnings + 1))
    fi
  fi

  collect_data_targets
  if [[ "${#DATA_TARGETS[@]}" -gt 0 ]]; then
    doctor_row "[INFO]" "Known data paths" "${#DATA_TARGETS[@]} discovered"
  fi

  printf "\nTroubleshooting hints\n"
  printf "  - Stable prebuilt:   ./install.sh --prebuilt --version v1.2.1\n"
  printf "  - Latest main build: ./install.sh --source\n"
  printf "  - PATH repair:       ./install.sh --repair-path\n"
  printf "  - About:             ./install.sh --about\n"

  printf "\nDoctor summary: %s failure(s), %s warning(s)\n" "$failures" "$warnings"
  if [[ "$failures" -gt 0 ]]; then
    warn "Doctor found blocking issues."
    return 1
  fi
  info "Doctor found no blocking issues."
}

run_path_repair() {
  local candidate_path candidate_dir resolved_prefix
  candidate_path=""
  candidate_dir=""

  if command -v bsj >/dev/null 2>&1; then
    candidate_path="$(command -v bsj)"
    candidate_dir="$(dirname "$candidate_path")"
  else
    resolved_prefix="$(effective_prebuilt_prefix)"
    if [[ -n "$BIN_DIR" ]]; then
      candidate_path="$(expand_home_path "$BIN_DIR")/bsj"
      if [[ -x "$candidate_path" ]]; then
        candidate_dir="$(dirname "$candidate_path")"
      fi
    fi
    if [[ -z "$candidate_dir" ]]; then
      candidate_path="$resolved_prefix/bin/bsj"
      if [[ -x "$candidate_path" ]]; then
        candidate_dir="$(dirname "$candidate_path")"
      fi
    fi
    if [[ -z "$candidate_dir" ]]; then
      candidate_path="${CARGO_HOME:-$HOME/.cargo}/bin/bsj"
      if [[ -x "$candidate_path" ]]; then
        candidate_dir="$(dirname "$candidate_path")"
      fi
    fi
  fi

  if [[ -z "$candidate_dir" ]]; then
    resolved_prefix="$(effective_prebuilt_prefix)"
    candidate_dir="${BIN_DIR:-$resolved_prefix/bin}"
    warn "No bsj binary found yet; adding expected install bin dir: $candidate_dir"
  fi

  print_installer_banner
  info "Repairing PATH integration for $candidate_dir"
  persist_path_update "$candidate_dir"
  if [[ ":$PATH:" == *":$candidate_dir:"* ]]; then
    info "Current shell already includes $candidate_dir."
  else
    printf "PATH updates were written to your shell config files.\n"
    printf "Open a new terminal window/tab to load them automatically.\n"
  fi

  if [[ -n "$candidate_path" && -x "$candidate_path" ]]; then
    print_install_version_summary "$candidate_path"
  else
    warn "Install bsj first, then re-run --repair-path if needed."
  fi
}

ensure_path_hint() {
  local path_entry="$1"
  if [[ ":$PATH:" == *":$path_entry:"* ]]; then
    return
  fi

  warn "Install finished, but this shell cannot find bsj yet ($path_entry is not on PATH)."
  persist_path_update "$path_entry"
  export PATH="$path_entry:$PATH"
  info "Added $path_entry to PATH for this installer session."
  printf "PATH updates were written to your shell config files.\n"
  printf "Open a new terminal window/tab to load them automatically.\n"
}

print_install_version_summary() {
  local installed_path="$1"
  local reported_version="" active_path=""

  if [[ -x "$installed_path" ]]; then
    reported_version="$("$installed_path" --version 2>/dev/null || true)"
    if [[ -n "$reported_version" ]]; then
      printf "%sInstalled version:%s %s\n" "$GREEN$BOLD" "$RESET" "$reported_version"
    fi
  fi

  if active_path="$(command -v bsj 2>/dev/null)"; then
    printf "%sActive bsj path:%s %s\n" "$GREEN$BOLD" "$RESET" "$active_path"
    if [[ "$active_path" != "$installed_path" ]]; then
      warn "PATH resolves bsj to a different location than the newly installed binary."
      warn "Open a new shell or run the installed path directly: $installed_path"
    fi
  else
    warn "bsj is not currently discoverable on PATH in this shell."
  fi
}

append_path_line_if_missing() {
  local target_file="$1"
  local marker="$2"
  local line="$3"
  local needle="$4"

  if ! mkdir -p "$(dirname "$target_file")"; then
    warn "Could not create shell config directory for $target_file"
    return 1
  fi
  if ! touch "$target_file"; then
    warn "Could not update shell config file $target_file"
    return 1
  fi
  if grep -Fq "$needle" "$target_file"; then
    return 1
  fi

  if [[ -s "$target_file" ]]; then
    printf '\n' >> "$target_file"
  fi
  printf "%s\n%s\n" "$marker" "$line" >> "$target_file"
  return 0
}

persist_path_update() {
  local path_entry="$1"
  local marker added existing target_file line
  local -a shell_target_files
  marker="# Added by bsj installer"
  added=0
  existing=0

  line="[[ \":\$PATH:\" != *\":$path_entry:\"* ]] && export PATH=\"$path_entry:\$PATH\""
  shell_target_files=("$HOME/.zprofile" "$HOME/.zshrc" "$HOME/.bash_profile" "$HOME/.bashrc")
  for target_file in "${shell_target_files[@]}"; do
    if [[ -f "$target_file" ]] && grep -Fq "$path_entry" "$target_file"; then
      existing=1
      continue
    fi
    if append_path_line_if_missing "$target_file" "$marker" "$line" "$path_entry"; then
      info "Added PATH update to $target_file"
      added=1
    fi
  done

  target_file="$HOME/.config/fish/config.fish"
  line="contains -- \"$path_entry\" \$PATH; or fish_add_path -m \"$path_entry\""
  if [[ -f "$target_file" ]] && grep -Fq "$path_entry" "$target_file"; then
    existing=1
  elif append_path_line_if_missing "$target_file" "$marker" "$line" "$path_entry"; then
    info "Added PATH update to $target_file"
    added=1
  fi

  if [[ "$added" -eq 0 ]]; then
    if [[ "$existing" -eq 1 ]]; then
      info "PATH update already present in shell config."
    else
      warn "Could not write PATH update automatically."
    fi
  fi
}

print_post_install_summary() {
  local installed_bin="$1"
  local help_ref="$2"

  cat <<EOF

Install complete.

Launch:
  $installed_bin

First-run flow:
  1) Launch BlueScreen Journal (setup wizard opens automatically if needed)
  2) Start typing your entry right away
  3) Press F2 to save, or type **save** then Enter for quick-save + next entry
  4) Press Esc (or Ctrl+O) to open menus: FILE/EDIT/SEARCH/GO/TOOLS/SETUP/HELP

Reference:
  $help_ref

Troubleshooting:
  ./install.sh --doctor
  ./install.sh --repair-path
EOF
}

launch_bsj_from_installer() {
  local installed_bin="$1"

  if tty_input_available && command -v script >/dev/null 2>&1; then
    script -q /dev/null "$installed_bin"
    return $?
  fi

  if tty_input_available; then
    "$installed_bin" < /dev/tty > /dev/tty 2>&1
    return $?
  fi

  "$installed_bin"
}

maybe_prompt_post_install_menu() {
  local installed_bin="$1"
  local help_ref="$2"
  local selection normalized

  if ! tty_input_available; then
    print_post_install_summary "$installed_bin" "$help_ref"
    return
  fi

  while true; do
    cat <<EOF

Post-install menu
  1) Launch BlueScreen Journal now (recommended)
  2) Print Setup guide (menu-first flow)
  3) Print keyboard/menu cheat sheet
  4) Show command help
  5) Run health check (doctor) + PATH repair
  6) Exit installer
EOF
    read_line_interactive "Select [1-6]: " || return
    selection="$REPLY"
    normalized="$(printf "%s" "$selection" | tr '[:upper:]' '[:lower:]')"
    case "$normalized" in
      ""|1)
        if ! launch_bsj_from_installer "$installed_bin"; then
          warn "Launch failed from installer. Run directly: $installed_bin"
        fi
        return
        ;;
      2)
        "$installed_bin" guide setup || "$installed_bin" guide quickstart || true
        ;;
      3)
        "$installed_bin" guide quickstart | sed -n '1,120p'
        ;;
      4)
        "$installed_bin" --help | sed -n '1,80p'
        ;;
      5)
        "$installed_bin" doctor || true
        run_path_repair
        ;;
      6|q|quit|exit)
        return
        ;;
      *)
        warn "Please choose 1-6."
        ;;
    esac
  done
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
  if [[ "${#args[@]}" -gt 0 ]]; then
    printf '%s\n' "${args[@]}"
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
    if [[ "$source" == "$output" ]]; then
      return
    fi
    ln "$source" "$output" 2>/dev/null || cp "$source" "$output"
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
    if [[ "$checksum_source" == "$output" ]]; then
      return 0
    fi
    ln "$checksum_source" "$output" 2>/dev/null || cp "$checksum_source" "$output"
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
  tmp_dir="$(make_temp_dir bsj-install)"
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
  [[ -x "$bundle_dir/bin/bsj" ]] || die "Bundled binary not found in archive"

  info "Installing from downloaded release bundle"
  local original_root="$ROOT_DIR"
  ROOT_DIR="$bundle_dir"
  if ! install_prebuilt; then
    ROOT_DIR="$original_root"
    return 1
  fi
  ROOT_DIR="$original_root"
}

bootstrap_source_install() {
  require_command curl
  require_command tar

  local tmp_dir source_archive_url source_archive_path source_dir ref_label
  tmp_dir="$(make_temp_dir bsj-source-install)"
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
  if [[ "${#delegate_args[@]}" -gt 0 ]]; then
    "$source_dir/install.sh" --source "${delegate_args[@]}"
  else
    "$source_dir/install.sh" --source
  fi
}

install_prebuilt() {
  if ! local_bundle_root; then
    bootstrap_prebuilt_install
    return
  fi

  local prefix
  prefix="$(effective_prebuilt_prefix)"
  local final_bin_dir="${BIN_DIR:-$prefix/bin}"
  local final_doc_dir="${DOC_DIR:-$prefix/share/doc/bsj}"
  local final_man_dir="${MAN_DIR:-$prefix/share/man/man1}"
  local final_example_dir="$prefix/share/bsj/examples"

  if [[ -z "$PREFIX" ]]; then
    info "Selected install prefix: $prefix"
  fi

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

  persist_path_update "$final_bin_dir"
  ensure_path_hint "$final_bin_dir"
  print_install_version_summary "$final_bin_dir/bsj"
  maybe_prompt_post_install_menu "$final_bin_dir/bsj" "man $final_man_dir/bsj.1"
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

  persist_path_update "$cargo_bin_dir"
  ensure_path_hint "$cargo_bin_dir"
  print_install_version_summary "$bsj_bin"
  maybe_prompt_post_install_menu "$bsj_bin" "$bsj_bin --help"
}

maybe_prompt_action_menu

if [[ -t 1 ]]; then
  print_installer_banner
fi

if [[ "$ACTION" == "uninstall" || "$ACTION" == "factory_reset" ]]; then
  if [[ "$MODE" != "auto" ]]; then
    warn "--source/--prebuilt is ignored for uninstall modes."
  fi
  if [[ -n "$ARCHIVE_SOURCE" ]]; then
    warn "--archive is ignored for uninstall modes."
  fi
  if [[ "$SKIP_CHECKSUM" -eq 1 ]]; then
    warn "--skip-checksum is ignored for uninstall modes."
  fi
fi

case "$ACTION" in
  install)
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
    ;;
  uninstall)
    run_uninstall
    ;;
  factory_reset)
    run_factory_reset
    ;;
  doctor)
    run_doctor
    ;;
  path_repair)
    run_path_repair
    ;;
  *)
    die "Unsupported installer action"
    ;;
esac
