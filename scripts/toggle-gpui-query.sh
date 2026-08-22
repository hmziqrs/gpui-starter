#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CARGO_TOML="$ROOT_DIR/Cargo.toml"
GPUI_QUERY_PATH="${GPUI_QUERY_PATH:-/Users/hmziq/fo/gpui-query}"

MARKER_START="# >>> gpui-query-local >>>"
MARKER_END="# <<< gpui-query-local <<<"
VENDORED='gpui-query = { path = "crates/gpui-query" }'
LOCAL_LINE="gpui-query = { path = \"$GPUI_QUERY_PATH/crates/gpui-query\" }"

usage() {
  cat <<EOF
Usage: just gpui-query-local | just gpui-query-cratesio | just gpui-query-status

Toggles the gpui-query source between the live standalone checkout
($GPUI_QUERY_PATH) and the in-repo vendored copy (crates/gpui-query =
published 0.1.4 + release-profile AppContext import fix) by flipping
comments inside the [patch.crates-io] marker block of Cargo.toml.

Exactly ONE of the two lines is active at a time — a duplicate
gpui-query key in [patch.crates-io] is a TOML parse error.

Commands:
  local     Use the standalone path checkout (active dev of gpui-query).
  cratesio  Use the vendored patched copy (default; run before committing).
            Named for symmetry: it plays the role the registry used to.
  status    Print the currently active source.

Env:
  GPUI_QUERY_PATH  Override the standalone checkout root (default: $GPUI_QUERY_PATH)
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

# Rewrites the marker block so exactly one of the two patch lines is active.
# The vendored line is identified by its relative path; any other
# `gpui-query = { path = ... }` line in the block is treated as the
# standalone-checkout line and regenerated with the current GPUI_QUERY_PATH.
toggle() {
  local mode="$1"
  awk -v mode="$mode" -v start="$MARKER_START" -v end="$MARKER_END" \
      -v vendored="$VENDORED" -v local_line="$LOCAL_LINE" '
    $0 == start { in_block=1; print; next }
    $0 == end   { in_block=0; print; next }
    in_block && $0 ~ /gpui-query = \{ path = "crates\/gpui-query" \}/ {
      print (mode == "cratesio") ? vendored : "# " vendored
      next
    }
    in_block && $0 ~ /^#? ?gpui-query = \{ path = / {
      print (mode == "local") ? local_line : "# " local_line
      next
    }
    { print }
  ' "$CARGO_TOML" > "$CARGO_TOML.tmp" && mv "$CARGO_TOML.tmp" "$CARGO_TOML"
}

status() {
  case "$(awk -v start="$MARKER_START" -v end="$MARKER_END" '
    $0 == start { in_block=1; next }
    $0 == end   { in_block=0; next }
    in_block && /^gpui-query = \{ path = "crates\/gpui-query" \}/ { v=1 }
    in_block && /^gpui-query = \{ path = / && !/"crates\/gpui-query"/ { l=1 }
    END { print (l ? "local" : (v ? "vendored" : "unknown")) }
  ' "$CARGO_TOML")" in
    local)    echo "gpui-query source: LOCAL ($GPUI_QUERY_PATH)" ;;
    vendored) echo "gpui-query source: vendored crates/gpui-query (0.1.4 + release-profile fix)" ;;
    *)        echo "gpui-query source: UNKNOWN (marker block has no active line)" >&2; exit 1 ;;
  esac
}

case "${1:-status}" in
  local)
    toggle local
    echo "Toggled to LOCAL path: $GPUI_QUERY_PATH"
    echo "Reminder: run 'just gpui-query-cratesio' before committing."
    ;;
  cratesio)
    toggle cratesio
    echo "Toggled to vendored copy (crates/gpui-query, 0.1.4 + fix)"
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
