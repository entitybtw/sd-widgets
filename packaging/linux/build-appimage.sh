#!/usr/bin/env bash
# Build Linux AppImages for sdw-deck and sdw-daemon.
#
#   packaging/linux/build-appimage.sh [deck|daemon|all]
#
# Requires a finished release build, or builds it. appimagetool is looked up in
# PATH, then in ~/.cache/sdw-widgets/tools; if missing it is downloaded from the
# official continuous release.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"

APP="${1:-all}"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
ARCH="$(uname -m)"
OUT="$ROOT/dist"
mkdir -p "$OUT"

echo "==> building release binaries"
cargo build --release -p sdw-deck -p sdw-daemon

TOOL_DIR="${XDG_CACHE_HOME:-$HOME/.cache}/sdw-widgets/tools"
mkdir -p "$TOOL_DIR"
APPIMAGETOOL="${APPIMAGETOOL:-$(command -v appimagetool || true)}"
if [ -z "$APPIMAGETOOL" ]; then
  APPIMAGETOOL="$TOOL_DIR/appimagetool-$ARCH.AppImage"
  if [ ! -x "$APPIMAGETOOL" ]; then
    echo "==> fetching appimagetool"
    curl -fL --retry 3 -o "$APPIMAGETOOL" \
      "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-$ARCH.AppImage"
    chmod +x "$APPIMAGETOOL"
  fi
fi

make_appdir() {
  local name="$1" bin="$2" desktop="$3"
  local dir="$ROOT/target/appimage-$name"
  rm -rf "$dir"
  mkdir -p "$dir/usr/bin" "$dir/usr/share/applications" \
           "$dir/usr/share/icons/hicolor/256x256/apps" "$dir/usr/share/licenses/$name"
  install -m 0755 "target/release/$bin" "$dir/usr/bin/$bin"
  install -m 0644 "packaging/linux/$desktop" "$dir/$desktop"
  install -m 0644 "packaging/linux/$desktop" "$dir/usr/share/applications/$desktop"
  install -m 0644 "assets/icons/sd-widgets-256.png" "$dir/sdw-widgets.png"
  install -m 0644 "assets/icons/sd-widgets-256.png" \
    "$dir/usr/share/icons/hicolor/256x256/apps/sdw-widgets.png"
  install -m 0644 LICENSE "$dir/usr/share/licenses/$name/LICENSE"
  cat > "$dir/AppRun" <<RUN
#!/bin/sh
HERE="\$(dirname "\$(readlink -f "\$0")")"
exec "\$HERE/usr/bin/$bin" "\$@"
RUN
  chmod +x "$dir/AppRun"
  echo "$dir"
}

package() {
  local name="$1" bin="$2" desktop="$3"
  local dir
  dir="$(make_appdir "$name" "$bin" "$desktop")"
  echo "==> packing $name"
  ARCH="$ARCH" "$APPIMAGETOOL" --appimage-extract-and-run \
    "$dir" "$OUT/$name-$VERSION-$ARCH.AppImage" 2>/dev/null || \
  ARCH="$ARCH" "$APPIMAGETOOL" "$dir" "$OUT/$name-$VERSION-$ARCH.AppImage"
}

case "$APP" in
  deck)  package sdw-deck sdw-deck sdw-deck.desktop ;;
  daemon) package sdw-daemon sdw-daemon sdw-daemon.desktop ;;
  all)
    package sdw-deck sdw-deck sdw-deck.desktop
    package sdw-daemon sdw-daemon sdw-daemon.desktop
    ;;
  *) echo "usage: $0 [deck|daemon|all]" >&2; exit 2 ;;
esac

echo "==> done: $OUT"
ls -lh "$OUT"
