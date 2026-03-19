#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

version="${1:-}"
if [[ -z "$version" ]]; then
  version="$(awk -F'"' '/^version = "/ { print $2; exit }' Cargo.toml)"
fi
if [[ -z "$version" ]]; then
  echo "unable to determine version (pass as first argument)" >&2
  exit 1
fi

if ! command -v curl >/dev/null 2>&1; then
  echo "curl is required" >&2
  exit 1
fi
if ! command -v osascript >/dev/null 2>&1; then
  echo "osascript is required for GUI smoke checks" >&2
  exit 1
fi

if [[ ! -d "/System/Applications/Utilities/Terminal.app" ]]; then
  echo "Terminal.app is not available" >&2
  exit 1
fi
if [[ ! -d "/Applications/iTerm.app" ]]; then
  echo "iTerm.app is not available (install with: brew install --cask iterm2)" >&2
  exit 1
fi

tmp_root="$(mktemp -d "${TMPDIR:-/tmp}/bsj-gui-smoke.XXXXXX")"
trap 'rm -rf "$tmp_root"' EXIT

install_home="$tmp_root/home"
install_prefix="$tmp_root/prefix"
mkdir -p "$install_home" "$install_prefix"

echo "==> Installing published v${version} bundle into temp prefix"
env HOME="$install_home" \
  bash -lc "curl -fsSL https://raw.githubusercontent.com/Awassee/bluescreenjournal/main/install.sh | bash -s -- --prebuilt --version v${version} --prefix \"$install_prefix\""

bsj_bin="$install_prefix/bin/bsj"
if [[ ! -x "$bsj_bin" ]]; then
  echo "installed bsj not found: $bsj_bin" >&2
  exit 1
fi

wait_for_done() {
  local marker="$1"
  local timeout_seconds="$2"
  local waited=0
  while [[ ! -f "$marker" ]]; do
    sleep 1
    waited=$((waited + 1))
    if [[ "$waited" -ge "$timeout_seconds" ]]; then
      return 1
    fi
  done
  return 0
}

run_terminal_smoke() {
  local app_label="$1"
  local app_name="$2"
  local log_file="$tmp_root/${app_label}.log"
  local done_file="$tmp_root/${app_label}.done"
  local runner_file="$tmp_root/${app_label}.sh"

  cat >"$runner_file" <<EOF
#!/usr/bin/env bash
set -euo pipefail
status=0
"$bsj_bin" --version >"$log_file" 2>&1 || status=\$?
"$bsj_bin" --help >>"$log_file" 2>&1 || status=\$?
echo "\$status" >"$done_file"
exit "\$status"
EOF
  chmod +x "$runner_file"

  if [[ "$app_name" == "Terminal" ]]; then
    osascript <<EOF >/dev/null
tell application "Terminal"
  activate
  do script "bash '$runner_file'"
end tell
EOF
  else
    osascript <<EOF >/dev/null
tell application "iTerm"
  activate
  if (count of windows) = 0 then
    set newWindow to (create window with default profile)
  else
    set newWindow to current window
  end if
  tell current session of newWindow
    write text "bash '$runner_file'"
  end tell
end tell
EOF
  fi

  if ! wait_for_done "$done_file" 45; then
    echo "${app_label}: timed out waiting for smoke command completion" >&2
    return 1
  fi

  local status
  status="$(cat "$done_file")"
  if [[ "$status" != "0" ]]; then
    echo "${app_label}: command failed (status $status)" >&2
    echo "---- ${app_label} log ----" >&2
    cat "$log_file" >&2 || true
    echo "-------------------------" >&2
    return 1
  fi

  echo "==> ${app_label} smoke passed"
  sed -n '1,12p' "$log_file"
}

run_terminal_smoke "terminal_app" "Terminal"
run_terminal_smoke "iterm2_app" "iTerm"

echo
echo "GUI terminal smoke passed for version v${version}"
