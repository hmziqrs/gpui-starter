#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CARGO_TOML="$ROOT_DIR/Cargo.toml"
GPUI_QUERY_PATH="${GPUI_QUERY_PATH:-/Users/hmziq/fo/gpui-query}"

MARKER_START="# >>> gpui-query-local >>>"
MARKER_END="# <<< gpui-query-local <<<"

usage() {
  cat <<EOF
Usage: just gpui-query-local | just gpui-query-cratesio | just gpui-query-status

Toggles the gpui-query / gpui-query-legacy source between a local path
checkout ($GPUI_QUERY_PATH) and crates.io by flipping comments inside the
[patch.crates-io] block of Cargo.toml.

Commands:
  local     Use local path checkout (for active development of gpui-query).
  cratesio  Use crates.io published version (default; run before committing).
  status    Print current source for both crates.

Env:
  GPUI_QUERY_PATH  Override the local checkout root (default: $GPUI_QUERY_PATH)
EOF
}

if [[ ! -f "$CARGO_TOML" ]]; then
  echo "error: Cargo.toml not found at $CARGO_TOML" >&2
  exit 1
fi

if ! grep -qF "$MARKER_START" "$CARGO_TOML"; then
  echo "error: marker block not found in $CARGO_TOML" >&2
  echo "       expected '$MARKER_START' inside [patch.crates-io]" >&2
  exit 1
fi

toggle() {
  local mode="$1"
  awk -v mode="$mode" -v start="$MARKER_START" -v end="$MARKER_END" '
    $0 == start { in_block=1; print; next }
    $0 == end   { in_block=0; print; next }
    in_block {
      if (mode == "local") {
        sub(/^# /, "", $0)
      } else if (mode == "cratesio") {
        if ($0 !~ /^# /) $0 = "# " $0
      }
      print
      next
    }
    { print }
  ' "$CARGO_TOML" > "$CARGO_TOML.tmp" && mv "$CARGO_TOML.tmp" "$CARGO_TOML"
}

status() {
  if awk -v start="$MARKER_START" -v end="$MARKER_END" '
    $0 == start { in_block=1; next }
    $0 == end   { in_block=0; next }
    in_block && /^gpui-query/ { found=1 }
    END { exit found ? 0 : 1 }
  ' "$CARGO_TOML"; then
    echo "gpui-query source: LOCAL ($GPUI_QUERY_PATH)"
  else
    echo "gpui-query source: crates.io (0.1.4)"
  fi
}

case "${1:-status}" in
  local)
    toggle local
    echo "Toggled to LOCAL path: $GPUI_QUERY_PATH"
    echo "Reminder: run 'just gpui-query-cratesio' before committing."
    ;;
  cratesio)
    toggle cratesio
    echo "Toggled to crates.io (0.1.4)"
    ;;
  status)
    status
    ;;
  -h|--help|help)
    usage
    ;;
  *)
    echo "error: unknown command '$1'" >&2
    usage >&2
    exit 1
    ;;
esac
