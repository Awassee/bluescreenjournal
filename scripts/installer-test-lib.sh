#!/usr/bin/env bash

run_with_timeout() {
  local timeout_seconds="$1"
  local label="$2"
  shift 2

  local pid status
  local start_seconds="$SECONDS"

  "$@" &
  pid=$!

  while kill -0 "$pid" >/dev/null 2>&1; do
    if (( SECONDS - start_seconds >= timeout_seconds )); then
      echo "Timed out after ${timeout_seconds}s: ${label}" >&2
      kill -TERM "$pid" >/dev/null 2>&1 || true
      sleep 2
      kill -KILL "$pid" >/dev/null 2>&1 || true
      wait "$pid" >/dev/null 2>&1 || true
      return 124
    fi
    sleep 1
  done

  wait "$pid"
  status=$?
  return "$status"
}

assert_log_contains() {
  local log_path="$1"
  local expected="$2"

  if ! grep -Fq "$expected" "$log_path"; then
    echo "Expected log to contain: $expected" >&2
    echo "Log path: $log_path" >&2
    tail -n 120 "$log_path" >&2 || true
    exit 1
  fi
}

assert_path_entry_count() {
  local file_path="$1"
  local path_entry="$2"
  local expected_count="$3"
  local actual_count

  actual_count="$(grep -Foc "$path_entry" "$file_path" || true)"
  if [[ "$actual_count" != "$expected_count" ]]; then
    echo "Expected $path_entry to appear $expected_count time(s) in $file_path, found $actual_count" >&2
    cat "$file_path" >&2 || true
    exit 1
  fi
}
