#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP_NAME="GPUI Starter"
BUNDLE_ID="com.gpui-starter.app"
TARGET_DIR="$ROOT_DIR/target/release"
APP_DIR="$TARGET_DIR/$APP_NAME.app"
CONTENTS_DIR="$APP_DIR/Contents"
MACOS_DIR="$CONTENTS_DIR/MacOS"

# Accept VERSION as argument or read from Cargo.toml.
VERSION="${1:-}"
if [ -z "$VERSION" ]; then
    VERSION="$(grep -m1 '^version' "$ROOT_DIR/Cargo.toml" | head -1 | sed 's/.*"\([^"]*\)".*/\1/')"
fi

# Parse version into integer build number (strip dots, keep leading zeros for sort).
BUILD_NUMBER="$(echo "$VERSION" | tr -d '.')"

cargo build --manifest-path "$ROOT_DIR/Cargo.toml" --release

rm -rf "$APP_DIR"
mkdir -p "$MACOS_DIR"

cp "$TARGET_DIR/gpui-starter" "$MACOS_DIR/$APP_NAME"

cat > "$CONTENTS_DIR/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key>
  <string>en</string>
  <key>CFBundleExecutable</key>
  <string>$APP_NAME</string>
  <key>CFBundleIdentifier</key>
  <string>$BUNDLE_ID</string>
  <key>CFBundleName</key>
  <string>$APP_NAME</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>$VERSION</string>
  <key>CFBundleVersion</key>
  <string>$BUILD_NUMBER</string>
  <key>LSMinimumSystemVersion</key>
  <string>10.14</string>
  <key>NSHighResolutionCapable</key>
  <true/>
</dict>
</plist>
PLIST

echo "$APP_DIR"
