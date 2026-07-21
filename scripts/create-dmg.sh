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

# Create the DMG using hdiutil (UDZO = compressed). `hdiutil create -srcfolder`
# intermittently fails with "Resource busy" on GHA macOS runners when
# Spotlight/mds races the freshly-copied source. Retry a few times, detaching
# any half-built volume between attempts. (Spotlight is also disabled job-wide
# in release.yml.)
detach_built_volume() {
  for m in "/Volumes/${VOLUME_NAME}" "/Volumes/${VOLUME_NAME} 1"; do
    hdiutil detach "$m" -force >/dev/null 2>&1 || true
  done
}

attempt=0
until hdiutil create \
    -volname "$VOLUME_NAME" \
    -srcfolder "$STAGING_DIR" \
    -ov \
    -format UDZO \
    "$OUTPUT_DMG"; do
  attempt=$((attempt + 1))
  if [ "$attempt" -ge 5 ]; then
    echo "Error: hdiutil create failed after 5 attempts" >&2
    detach_built_volume
    exit 1
  fi
  echo "hdiutil attempt $attempt failed (likely Spotlight race); retrying in 10s..." >&2
  detach_built_volume
  rm -f "$OUTPUT_DMG"
  sleep 10
done

echo "DMG created: $OUTPUT_DMG"
