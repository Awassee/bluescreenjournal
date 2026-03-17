#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "Usage: ./scripts/scrub-binary-paths.sh PATH_TO_BINARY" >&2
  exit 1
fi

BINARY_PATH="$1"
[[ -f "$BINARY_PATH" ]] || {
  echo "Binary not found: $BINARY_PATH" >&2
  exit 1
}

perl -0pi -e '
sub scrub {
  my ($match, $replacement) = @_;
  my $pad = length($match) - length($replacement);
  die "replacement longer than match" if $pad < 0;
  return $replacement . ("_" x $pad);
}

s{(/home/[^/]+/\.cargo/registry/src/index\.crates\.io-[0-9a-f]+/)}{scrub($1, "/cargo/registry/src/index.crates.io/")}ge;
s{(/private/tmp/rust-[^/]+/rustc-[^/]+-src/)}{scrub($1, "/rust-src/")}ge;
s{(/workspace/)}{scrub($1, "/srcroot/")}ge;
' "$BINARY_PATH"
