#!/usr/bin/env bash
#
# Install the gpui-starter desktop entry + icon for the CURRENT user (XDG dirs),
# so the desktop environment can launch the app, show its icon, and group its
# notifications with the right identity.
#
# A .desktop file + icon that ship INSIDE the release tarball are otherwise
# INERT — GNOME/KDE only see entries in ~/.local/share/applications and icons
# in ~/.local/share/icons/hicolor. Run this once after extracting the tarball:
#
#   ./install-linux.sh
#
# (Idempotent; safe to re-run. No sudo — installs per-user only.)
set -euo pipefail

APP_ID="com.gpui-starter.app"
ICON_NAME="gpui-starter"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Find an asset that ships either flat in the tarball (next to this script) or
# in the repo layout (resources/ next to scripts/).
find_asset() {
    for p in "$SCRIPT_DIR/$1" "$SCRIPT_DIR/resources/$1" "$SCRIPT_DIR/../resources/$1"; do
        if [ -f "$p" ]; then
            printf '%s' "$p"
            return 0
        fi
    done
    echo "Error: '$1' not found (looked in $SCRIPT_DIR, $SCRIPT_DIR/resources, $SCRIPT_DIR/../resources)" >&2
    return 1
}

DESKTOP_SRC="$(find_asset "${APP_ID}.desktop")"
ICON_SRC="$(find_asset "${ICON_NAME}.png")"

DATA_DIR="${XDG_DATA_HOME:-$HOME/.local/share}"
APPS_DIR="${DATA_DIR}/applications"
ICON_DIR="${DATA_DIR}/icons/hicolor/512x512/apps"

mkdir -p "$APPS_DIR" "$ICON_DIR"

install -Dm644 "$DESKTOP_SRC" "${APPS_DIR}/${APP_ID}.desktop"
install -Dm644 "$ICON_SRC" "${ICON_DIR}/${ICON_NAME}.png"

# Refresh the desktop + icon caches (best-effort; ignore if tools are absent).
if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$APPS_DIR" || true
fi
# gtk-update-icon-cache may be named with a version suffix.
GTK_ICC="$(command -v gtk-update-icon-cache-3.0 || command -v gtk-update-icon-cache || true)"
if [ -n "$GTK_ICC" ]; then
    "$GTK_ICC" -f "${DATA_DIR}/icons/hicolor" || true
fi

echo "Installed ${APP_ID}.desktop  -> ${APPS_DIR}"
echo "Installed ${ICON_NAME}.png  -> ${ICON_DIR}"
echo "Done. The desktop should now group the window + notifications under 'GPUI Starter'."
