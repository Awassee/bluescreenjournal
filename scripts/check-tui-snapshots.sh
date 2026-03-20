#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
EXPECTED_DIR="$ROOT_DIR/tests/snapshots/tui"
ARTIFACT_DIR="${BSJ_TUI_SNAPSHOT_ARTIFACT_DIR:-$ROOT_DIR/artifacts/tui-snapshots}"
ACTUAL_DIR="$ARTIFACT_DIR/actual"
DIFF_DIR="$ARTIFACT_DIR/diff"
HTML_INDEX="$ARTIFACT_DIR/index.html"
SUMMARY_MD="$ARTIFACT_DIR/SUMMARY.md"
ACCEPT=0

usage() {
  cat <<'EOF'
check-tui-snapshots.sh

Usage:
  ./scripts/check-tui-snapshots.sh [--accept]

Generates deterministic text snapshots for core TUI screens, compares them to the
committed expectations under tests/snapshots/tui, and writes actual/diff artifacts
under artifacts/tui-snapshots.

Options:
  --accept   Replace committed expected snapshots with the newly generated ones.
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --help|-h)
      usage
      exit 0
      ;;
    --accept)
      ACCEPT=1
      shift
      ;;
    *)
      echo "Unknown option: $1" >&2
      exit 1
      ;;
  esac
done

rm -rf "$ARTIFACT_DIR"
mkdir -p "$ACTUAL_DIR" "$DIFF_DIR" "$EXPECTED_DIR"

html_escape_file() {
  sed \
    -e 's/&/\&amp;/g' \
    -e 's/</\&lt;/g' \
    -e 's/>/\&gt;/g' \
    "$1"
}

echo "==> Generating TUI screen snapshots"
BSJ_TUI_SNAPSHOT_DIR="$ACTUAL_DIR" \
  CARGO_INCREMENTAL=0 cargo test --all-targets "tui::app::tests::write_named_ui_snapshots" -- --exact --ignored

if [[ "$ACCEPT" -eq 1 ]]; then
  echo "==> Accepting new snapshots"
  rm -f "$EXPECTED_DIR"/*.txt
  cp "$ACTUAL_DIR"/*.txt "$EXPECTED_DIR"/
fi

status=0

for actual in "$ACTUAL_DIR"/*.txt; do
  name="$(basename "$actual")"
  expected="$EXPECTED_DIR/$name"
  if [[ ! -f "$expected" ]]; then
    echo "Missing expected snapshot: $expected" >&2
    status=1
    continue
  fi
  if ! diff -u "$expected" "$actual" > "$DIFF_DIR/$name.diff"; then
    echo "Snapshot mismatch: $name" >&2
    status=1
  else
    rm -f "$DIFF_DIR/$name.diff"
  fi
done

for expected in "$EXPECTED_DIR"/*.txt; do
  name="$(basename "$expected")"
  if [[ ! -f "$ACTUAL_DIR/$name" ]]; then
    echo "Expected snapshot was not regenerated: $name" >&2
    status=1
  fi
done

cat > "$ARTIFACT_DIR/README.txt" <<EOF
BlueScreen Journal TUI snapshot artifacts

Expected snapshots: $EXPECTED_DIR
Actual snapshots:   $ACTUAL_DIR
Diffs:              $DIFF_DIR
Preview:            $HTML_INDEX

Files are plain-text terminal screen captures for core nostalgic UI states.
EOF

total_snapshots="$(find "$EXPECTED_DIR" -maxdepth 1 -type f -name '*.txt' | wc -l | tr -d ' ')"
diff_count="$(find "$DIFF_DIR" -maxdepth 1 -type f -name '*.diff' | wc -l | tr -d ' ')"
if [[ "$diff_count" -eq 0 ]]; then
  snapshot_result="PASS"
  snapshot_note="All committed nostalgia screens matched the generated output."
else
  snapshot_result="FAIL"
  snapshot_note="One or more nostalgia screens changed. Open the artifact preview before merging or releasing."
fi

cat > "$SUMMARY_MD" <<EOF
## Nostalgia Snapshot Review

- Result: **$snapshot_result**
- Screens checked: **$total_snapshots**
- Diffs found: **$diff_count**
- Preview file: \`artifacts/tui-snapshots/index.html\`
- Artifact bundle: \`bsj-qa-artifacts\`

$snapshot_note
EOF

{
  cat <<'EOF'
<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <title>BlueScreen Journal TUI Snapshot Preview</title>
  <style>
    :root {
      color-scheme: only light;
      --bg: #0b4fd1;
      --panel: #0a44b2;
      --panel-border: rgba(255,255,255,0.28);
      --text: #f5f7ff;
      --muted: rgba(245,247,255,0.78);
      --ok: #8ff0a4;
      --warn: #ffd166;
    }
    * { box-sizing: border-box; }
    body {
      margin: 0;
      padding: 24px;
      background: linear-gradient(180deg, #0a47be 0%, #0b57de 100%);
      color: var(--text);
      font-family: Menlo, Monaco, "Courier New", monospace;
    }
    h1, h2, h3, p { margin: 0; }
    .hero { margin-bottom: 24px; }
    .hero p { color: var(--muted); margin-top: 8px; max-width: 80ch; }
    .screen {
      background: rgba(8, 36, 98, 0.72);
      border: 1px solid var(--panel-border);
      border-radius: 10px;
      padding: 16px;
      margin-bottom: 18px;
      box-shadow: 0 18px 40px rgba(0,0,0,0.18);
    }
    .screen h2 { margin-bottom: 10px; font-size: 18px; }
    .status { font-size: 12px; margin-bottom: 12px; color: var(--muted); }
    .status.ok { color: var(--ok); }
    .status.diff { color: var(--warn); }
    .grid {
      display: grid;
      grid-template-columns: repeat(auto-fit, minmax(320px, 1fr));
      gap: 16px;
    }
    .pane {
      background: rgba(5, 24, 70, 0.72);
      border: 1px solid rgba(255,255,255,0.18);
      border-radius: 8px;
      overflow: hidden;
    }
    .pane h3 {
      padding: 10px 12px;
      font-size: 13px;
      color: var(--muted);
      border-bottom: 1px solid rgba(255,255,255,0.15);
      background: rgba(255,255,255,0.04);
    }
    pre {
      margin: 0;
      padding: 12px;
      min-height: 120px;
      overflow: auto;
      white-space: pre;
      background: var(--bg);
      color: var(--text);
      line-height: 1.15;
      font-size: 12px;
    }
    details {
      margin-top: 12px;
      background: rgba(5,24,70,0.72);
      border: 1px solid rgba(255,255,255,0.18);
      border-radius: 8px;
      padding: 10px 12px;
    }
    summary { cursor: pointer; color: var(--muted); }
    details pre { margin-top: 10px; }
  </style>
</head>
<body>
  <div class="hero">
    <h1>BlueScreen Journal TUI Snapshot Preview</h1>
    <p>Expected vs actual nostalgic screen captures from the automated QA pass. Open this file from the CI artifact bundle for a visual review without reconstructing the terminal locally.</p>
  </div>
EOF

  for expected in "$EXPECTED_DIR"/*.txt; do
    name="$(basename "$expected")"
    actual="$ACTUAL_DIR/$name"
    diff_path="$DIFF_DIR/$name.diff"
    status_class="ok"
    status_label="MATCH"
    if [[ -f "$diff_path" ]]; then
      status_class="diff"
      status_label="DIFF"
    fi

    cat <<EOF
  <section class="screen">
    <h2>$name</h2>
    <div class="status $status_class">$status_label</div>
    <div class="grid">
      <div class="pane">
        <h3>Expected</h3>
        <pre>
EOF
    html_escape_file "$expected"
    cat <<EOF
        </pre>
      </div>
      <div class="pane">
        <h3>Actual</h3>
        <pre>
EOF
    if [[ -f "$actual" ]]; then
      html_escape_file "$actual"
    else
      printf '%s\n' "[missing actual snapshot]"
    fi
    cat <<EOF
        </pre>
      </div>
    </div>
EOF
    if [[ -f "$diff_path" ]]; then
      cat <<'EOF'
    <details>
      <summary>Unified diff</summary>
      <pre>
EOF
      html_escape_file "$diff_path"
      cat <<'EOF'
      </pre>
    </details>
EOF
    fi
    cat <<'EOF'
  </section>
EOF
  done

  cat <<'EOF'
</body>
</html>
EOF
} > "$HTML_INDEX"

if [[ "$status" -ne 0 ]]; then
  echo "TUI snapshot check failed. Review artifacts under $ARTIFACT_DIR" >&2
  exit 1
fi

echo "TUI snapshot check passed."
