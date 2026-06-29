#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP_NAME="GPUI Starter"
BUNDLE_ID="com.gpui-starter.app"
TARGET_DIR="$ROOT_DIR/target/release"
# Args: <version> [universal|aarch64|x86_64]  (arch defaults to universal)
VERSION="${1:-}"
ARCH="${2:-universal}"
if [ -z "$VERSION" ]; then
    VERSION="$(grep -m1 '^version' "$ROOT_DIR/Cargo.toml" | head -1 | sed 's/.*"\([^"]*\)".*/\1/')"
fi

# Parse version into integer build number (strip dots, keep leading zeros for sort).
BUILD_NUMBER="$(echo "$VERSION" | tr -d '.')"

ARM_BIN="$ROOT_DIR/target/aarch64-apple-darwin/release/gpui-starter"
X86_BIN="$ROOT_DIR/target/x86_64-apple-darwin/release/gpui-starter"
for b in "$ARM_BIN" "$X86_BIN"; do
  [ -f "$b" ] || { echo "Error: expected build output not found: $b" >&2; exit 1; }
done

# Stage each arch under its own dir so the three .app bundles don't collide;
# the .app inside is always "GPUI Starter.app" (the name users see in the DMG).
APP_DIR="$TARGET_DIR/dist/$ARCH/$APP_NAME.app"
CONTENTS_DIR="$APP_DIR/Contents"
MACOS_DIR="$CONTENTS_DIR/MacOS"

rm -rf "$APP_DIR"
mkdir -p "$MACOS_DIR"

# universal = fat Mach-O (arm64 + x86_64 slices, the default download); the
# slim builds copy a single arch for users who want a smaller download.
case "$ARCH" in
  universal)
    lipo -create "$ARM_BIN" "$X86_BIN" -output "$MACOS_DIR/$APP_NAME"
    ;;
  aarch64)
    cp "$ARM_BIN" "$MACOS_DIR/$APP_NAME"
    ;;
  x86_64)
    cp "$X86_BIN" "$MACOS_DIR/$APP_NAME"
    ;;
  *)
    echo "Error: unknown arch '$ARCH' (expected universal|aarch64|x86_64)" >&2
    exit 1
    ;;
esac
echo "$ARCH .app architectures: $(lipo -archs "$MACOS_DIR/$APP_NAME" 2>&1 || echo unknown)" >&2

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
