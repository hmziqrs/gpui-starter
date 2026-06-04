#!/usr/bin/env bash
set -euo pipefail

# Usage: generate-manifest.sh <VERSION> <GITHUB_REPO> [OUTPUT_FILE]
# Example: generate-manifest.sh 0.2.0 myorg/gpui-app manifest.json
# If OUTPUT_FILE is omitted, writes to stdout.
#
# Optional environment variables:
#   UPDATE_SIGNING_KEY  — base64-encoded Ed25519 private key for signing assets.
#                         If not set, the signature field is left empty.

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

# Compute Ed25519 signature of the asset's SHA-256 hash.
# The signing key is a base64-encoded 64-byte Ed25519 private key (seed || public key).
SIGNATURE=""

if [ -n "${UPDATE_SIGNING_KEY:-}" ]; then
    echo "Signing manifest with Ed25519..." >&2

    # Download the asset to compute its hash.
    TMP_ASSET="$(mktemp)"
    trap 'rm -f "$TMP_ASSET"' EXIT

    curl -sfL -o "$TMP_ASSET" "$DMG_URL" || {
        echo "Warning: Failed to download asset for signing — signature will be empty" >&2
        TMP_ASSET=""
    }

    if [ -n "$TMP_ASSET" ] && [ -f "$TMP_ASSET" ]; then
        # Compute SHA-256 of the downloaded file.
        FILE_HASH="$(shasum -a 256 "$TMP_ASSET" | awk '{print $1}')"

        # Decode the signing key and sign the hash using OpenSSL (or python).
        # We use python3 + PyNaCl-like approach via the ed25519 module if available,
        # falling back to openssl if not.
        SIGNATURE="$(python3 -c "
import base64, hashlib, json, os, sys

try:
    from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
    from cryptography.hazmat.primitives.serialization import Encoding, PublicFormat
    signing_key_b64 = os.environ.get('UPDATE_SIGNING_KEY', '')
    if not signing_key_b64:
        print('', end='')
        sys.exit(0)
    key_bytes = base64.b64decode(signing_key_b64)
    private_key = Ed25519PrivateKey.from_private_bytes(key_bytes[:32])
    file_hash = bytes.frombytes(bytes.fromhex('$FILE_HASH')) if len('$FILE_HASH') == 64 else b''
    # Actually, sign the raw hash bytes
    file_hash_bytes = bytes.fromhex('$FILE_HASH')
    sig = private_key.sign(file_hash_bytes)
    print(base64.b64encode(sig).decode(), end='')
except Exception as e:
    print(f'Warning: signing failed: {e}', file=sys.stderr)
    print('', end='')
" 2>/dev/null || true)"

        if [ -z "$SIGNATURE" ]; then
            echo "Warning: Ed25519 signing failed — signature will be empty" >&2
        else
            echo "Ed25519 signature generated: ${SIGNATURE:0:20}..." >&2
        fi
    fi
fi

# Generate the manifest JSON matching the UpdateManifest struct in updater.rs:
#   { version, release_notes, platforms: { "<key>": { url, signature, size } } }
MANIFEST="$(jq -n \
    --arg version "$VERSION" \
    --arg notes "$RELEASE_NOTES" \
    --arg dmg_url "$DMG_URL" \
    --argjson dmg_size "$DMG_SIZE" \
    --arg signature "$SIGNATURE" \
    '{
        "version": $version,
        "release_notes": $notes,
        "platforms": {
            "macos-aarch64": {
                "url": $dmg_url,
                "signature": $signature,
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
