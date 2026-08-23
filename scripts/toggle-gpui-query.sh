#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CARGO_TOML="$ROOT_DIR/Cargo.toml"
GPUI_QUERY_PATH="${GPUI_QUERY_PATH:-/Users/hmziq/fo/gpui-query}"

MARKER_START="# >>> gpui-query-local >>>"
MARKER_END="# <<< gpui-query-local <<<"
# DEFAULT: published 0.1.4 + release-profile AppContext fix (see Cargo.toml).
DEFAULT_LINE='gpui-query = { git = "https://github.com/hmziqagent/gpui-query", rev = "f84eac4be3c46c1d7e5d8cffb8b6a898d4526f73" }'
LOCAL_LINE="gpui-query = { path = \"$GPUI_QUERY_PATH/crates/gpui-query\" }"

usage() {
  cat <<EOF
Usage: just gpui-query-local | just gpui-query-cratesio | just gpui-query-status

Toggles the gpui-query [patch.crates-io] source between the live standalone
checkout ($GPUI_QUERY_PATH) and the default git rev (0.1.4 + the
release-profile AppContext import fix that no published version carries).

Exactly ONE of the two lines in the marker block is active at a time —
a duplicate gpui-query key in [patch.crates-io] is a TOML parse error.

Commands:
  local     Use the standalone path checkout (active dev of gpui-query).
  cratesio  Use the default git rev (run before committing). Named for
            symmetry; the registry itself does not build in release profile.
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
# The git line is identified by its github.com URL; any other
# `gpui-query = { path = ... }` line is treated as the standalone-checkout
# line and regenerated with the current GPUI_QUERY_PATH.
toggle() {
  local mode="$1"
  awk -v mode="$mode" -v start="$MARKER_START" -v end="$MARKER_END" \
      -v default_line="$DEFAULT_LINE" -v local_line="$LOCAL_LINE" '
    $0 == start { in_block=1; print; next }
    $0 == end   { in_block=0; print; next }
    in_block && /gpui-query = \{ git = / {
      print (mode == "cratesio") ? default_line : "# " default_line
      next
    }
    in_block && /^#? ?gpui-query = \{ path = / {
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
    in_block && /^gpui-query = \{ git = / { d=1 }
    in_block && /^gpui-query = \{ path = / { l=1 }
    END { print (l ? "local" : (d ? "git" : "unknown")) }
  ' "$CARGO_TOML")" in
    local) echo "gpui-query source: LOCAL ($GPUI_QUERY_PATH)" ;;
    git)   echo "gpui-query source: git hmziqagent/gpui-query@f84eac4 (0.1.4 + release fix)" ;;
    *)     echo "gpui-query source: UNKNOWN (marker block has no active line)" >&2; exit 1 ;;
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
    echo "Toggled to default git rev (0.1.4 + release-profile fix)"
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
