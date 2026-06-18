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

# Authenticated curl: passes GH_TOKEN when present (private repos, and to dodge
# the shared-IP 60 req/hr anonymous limit). No-op for public repos w/o a token.
gh_curl() {
    if [ -n "${GH_TOKEN:-}" ]; then
        curl -H "Authorization: Bearer ${GH_TOKEN}" -H "Accept: application/vnd.github+json" "$@"
    else
        curl "$@"
    fi
}

# Fetch release metadata from GitHub.
RELEASE_JSON="$(gh_curl -sf "$API_URL")"

if [ -z "$RELEASE_JSON" ]; then
    echo "Error: Failed to fetch release for v${VERSION} from ${GITHUB_REPO}" >&2
    exit 1
fi

# Extract the release body as notes. Do NOT pre-escape here: the downstream
# `jq -n --arg notes` (below) is the single source of truth for JSON escaping.
# The old manual gsub chain double-escaped and turned "\n" into literal "\\n".
RELEASE_NOTES="$(echo "$RELEASE_JSON" | jq -r '.body // ""')"

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

    if ! gh_curl -sfL -o "$TMP_ASSET" "$DMG_URL"; then
        echo "Warning: Failed to download asset for signing — signature will be empty" >&2
        TMP_ASSET=""
    fi

    if [ -n "$TMP_ASSET" ] && [ -f "$TMP_ASSET" ]; then
        # Compute SHA-256 of the downloaded file.
        FILE_HASH="$(shasum -a 256 "$TMP_ASSET" | awk '{print $1}')"

        # Decode the base64 Ed25519 seed and sign the asset's SHA-256 hash.
        # FILE_HASH is passed via env (not shell interpolation) to avoid
        # injection/quoting pitfalls. There is intentionally NO `2>/dev/null ||
        # true`: any signing failure must surface — the hard guard below turns
        # an empty signature into a hard error (this block only runs when
        # UPDATE_SIGNING_KEY is set). NOTE: `cryptography` is NOT on ubuntu-latest
        # by default; the release workflow pip-installs it before calling us.
        SIGNATURE="$(FILE_HASH="$FILE_HASH" python3 -c '
import base64, os, sys

try:
    from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
    signing_key_b64 = os.environ.get("UPDATE_SIGNING_KEY", "")
    if not signing_key_b64:
        print("", end="")
        sys.exit(0)
    hexhash = os.environ.get("FILE_HASH", "")
    if len(hexhash) != 64:
        raise ValueError(f"bad sha256 length: {len(hexhash)}")
    key_bytes = base64.b64decode(signing_key_b64)
    private_key = Ed25519PrivateKey.from_private_bytes(key_bytes[:32])
    sig = private_key.sign(bytes.fromhex(hexhash))
    print(base64.b64encode(sig).decode(), end="")
except Exception as e:
    print(f"signing failed: {e}", file=sys.stderr)
    print("", end="")
')"

        if [ -z "$SIGNATURE" ]; then
            echo "Error: UPDATE_SIGNING_KEY is set but Ed25519 signing produced an empty signature" >&2
            exit 1
        fi
        echo "Ed25519 signature generated: ${SIGNATURE:0:20}..." >&2
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
