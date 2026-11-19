#!/usr/bin/env bash
# Build a macOS .app bundle for sdw-daemon and wrap it in a DMG.
#
#   packaging/macos/build-dmg.sh [arm64|x86_64]
#
# Requires the matching Rust target and Xcode command line tools (hdiutil).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"

ARCH="${1:-$(uname -m)}"
case "$ARCH" in
  arm64|aarch64) TARGET="aarch64-apple-darwin"; APP_ARCH="arm64" ;;
  x86_64)        TARGET="x86_64-apple-darwin";  APP_ARCH="x86_64" ;;
  *) echo "usage: $0 [arm64|x86_64]" >&2; exit 2 ;;
esac

VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
APP="sdw-daemon"
BUNDLE="$ROOT/target/macos/$APP.app"
DIST="$ROOT/dist"
mkdir -p "$DIST"

echo "==> building $APP $VERSION for $TARGET"
rustup target add "$TARGET" >/dev/null 2>&1 || true
cargo build --release -p sdw-daemon --target "$TARGET"

rm -rf "$BUNDLE"
mkdir -p "$BUNDLE/Contents/MacOS" "$BUNDLE/Contents/Resources"
install -m 0755 "target/$TARGET/release/$APP" "$BUNDLE/Contents/MacOS/$APP"
cp "assets/icons/sd-widgets-256.png" "$BUNDLE/Contents/Resources/sdw-widgets.png"

cat > "$BUNDLE/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>sd-widgets daemon</string>
  <key>CFBundleDisplayName</key><string>sd-widgets daemon</string>
  <key>CFBundleIdentifier</key><string>ru.entitybtw.sd-widgets-daemon</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleExecutable</key><string>$APP</string>
  <key>CFBundleIconFile</key><string>sdw-widgets.png</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>LSMinimumSystemVersion</key><string>12.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSLocalNetworkUsageDescription</key>
  <string>sd-widgets needs the local network to talk to your Steam Deck.</string>
</dict>
</plist>
PLIST

if [ -d "$BUNDLE/Contents/Resources/sdw-widgets.iconset" ]; then
  iconutil -c icns "$BUNDLE/Contents/Resources/sdw-widgets.iconset" \
    -o "$BUNDLE/Contents/Resources/sdw-widgets.icns" || true
fi

DMG="$DIST/sdw-daemon-$VERSION-macos-$APP_ARCH.dmg"
rm -f "$DMG"
STAGE="$ROOT/target/macos/dmg"
rm -rf "$STAGE"
mkdir -p "$STAGE"
cp -R "$BUNDLE" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
cp README.md LICENSE "$STAGE/"

echo "==> packing $DMG"
hdiutil create -volname "sd-widgets $VERSION" -srcfolder "$STAGE" -ov -format UDZO "$DMG"
echo "==> $DMG"
ls -lh "$DMG"
