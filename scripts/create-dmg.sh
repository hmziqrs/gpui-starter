#!/usr/bin/env bash
set -euo pipefail

# Usage: create-dmg.sh <INPUT_APP_DIR> <OUTPUT_DMG>
# Example: create-dmg.sh target/release/GPUI\ Starter.app target/release/GPUI-Starter-0.2.0.dmg

if [ $# -lt 2 ]; then
    echo "Usage: $0 <INPUT_APP_DIR> <OUTPUT_DMG>" >&2
    exit 1
fi

INPUT_APP_DIR="$1"
OUTPUT_DMG="$2"
VOLUME_NAME="GPUI Starter"

if [ ! -d "$INPUT_APP_DIR" ]; then
    echo "Error: App bundle not found at $INPUT_APP_DIR" >&2
    exit 1
fi

# Remove existing DMG if present.
rm -f "$OUTPUT_DMG"

# Create a temporary directory for the DMG staging area.
STAGING_DIR="$(mktemp -d)"
trap 'rm -rf "$STAGING_DIR"' EXIT

# Copy the .app bundle into the staging directory.
cp -R "$INPUT_APP_DIR" "$STAGING_DIR/"

# Create the DMG using hdiutil (UDZO = compressed).
hdiutil create \
    -volname "$VOLUME_NAME" \
    -srcfolder "$STAGING_DIR" \
    -ov \
    -format UDZO \
    "$OUTPUT_DMG"

echo "DMG created: $OUTPUT_DMG"
