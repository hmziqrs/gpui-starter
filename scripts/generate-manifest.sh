#!/usr/bin/env bash
set -euo pipefail

# Usage: generate-manifest.sh <VERSION> <GITHUB_REPO> [OUTPUT_FILE]
# Example: generate-manifest.sh 0.2.0 myorg/gpui-app manifest.json
# If OUTPUT_FILE is omitted, writes to stdout.

if [ $# -lt 2 ]; then
    echo "Usage: $0 <VERSION> <GITHUB_REPO> [OUTPUT_FILE]" >&2
    exit 1
fi

VERSION="$1"
GITHUB_REPO="$2"
OUTPUT_FILE="${3:-}"

API_URL="https://api.github.com/repos/${GITHUB_REPO}/releases/tags/v${VERSION}"

# Fetch release metadata from GitHub.
RELEASE_JSON="$(curl -sf "$API_URL")"

if [ -z "$RELEASE_JSON" ]; then
    echo "Error: Failed to fetch release for v${VERSION} from ${GITHUB_REPO}" >&2
    exit 1
fi

# Extract the release body as notes (escape for JSON embedding).
RELEASE_NOTES="$(echo "$RELEASE_JSON" | jq -r '.body // "" | gsub("\\\\"; "\\\\\\\\") | gsub("\""; "\\\"") | gsub("\n"; "\\n") | gsub("\t"; "\\t")')"

# Build the platforms object.
# Naming convention: GPUI-Starter-{version}.dmg -> macos-aarch64
DMG_NAME="GPUI-Starter-${VERSION}.dmg"

# Find the DMG asset in the release.
DMG_ASSET="$(echo "$RELEASE_JSON" | jq -r --arg name "$DMG_NAME" '.assets[] | select(.name == $name)')"

if [ -z "$DMG_ASSET" ]; then
    echo "Error: Asset '$DMG_NAME' not found in release v${VERSION}" >&2
    exit 1
fi

DMG_URL="$(echo "$DMG_ASSET" | jq -r '.browser_download_url')"
DMG_SIZE="$(echo "$DMG_ASSET" | jq -r '.size')"

# Generate the manifest JSON matching the UpdateManifest struct in updater.rs:
#   { version, release_notes, platforms: { "<key>": { url, signature, size } } }
MANIFEST="$(jq -n \
    --arg version "$VERSION" \
    --arg notes "$RELEASE_NOTES" \
    --arg dmg_url "$DMG_URL" \
    --argjson dmg_size "$DMG_SIZE" \
    '{
        "version": $version,
        "release_notes": $notes,
        "platforms": {
            "macos-aarch64": {
                "url": $dmg_url,
                "signature": "",
                "size": $dmg_size
            }
        }
    }')"

if [ -n "$OUTPUT_FILE" ]; then
    echo "$MANIFEST" > "$OUTPUT_FILE"
    echo "Manifest written to $OUTPUT_FILE" >&2
else
    echo "$MANIFEST"
fi
