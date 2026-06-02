#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP_DIR="$("$ROOT_DIR/scripts/macos-dev-app.sh")"
EXECUTABLE="$APP_DIR/Contents/MacOS/GPUI Starter"

# export RUST_LOG="${RUST_LOG:-gpui_starter::notifications=trace,user_notify=debug,notify_rust=debug,gpui_starter=info}"
export RUST_BACKTRACE=1
exec "$EXECUTABLE"
