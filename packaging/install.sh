#!/usr/bin/env bash
# sd-widgets installer — one command for the computer and one for the Deck.
#
#   curl -fsSL https://git.entitybtw.ru/entitybtw/sd-widgets/raw/branch/main/packaging/install.sh \
#     | bash -s -- daemon
#   curl -fsSL https://git.entitybtw.ru/entitybtw/sd-widgets/raw/branch/main/packaging/install.sh \
#     | bash -s -- deck
#
# The script downloads the newest AppImage from the release, installs a small
# launcher into ~/.local/bin, adds a desktop entry, and:
#   * daemon — installs and starts a systemd user service (or an XDG autostart
#     entry when systemd is not available);
#   * deck   — adds sd-widgets to the Steam library as a non-Steam shortcut with
#     library artwork, so it runs from Game Mode without extra steps.
#
# Options: --version V, --local FILE, --dir DIR, --no-steam, --steam-only,
#          --no-steam-restart, --no-service, --force, -h.
set -euo pipefail

BASE="${SDW_BASE:-https://git.entitybtw.ru/entitybtw/sd-widgets}"

APP=""
VERSION=""
LOCAL_FILE=""
PREFIX="${HOME}/.local"
NO_STEAM=0
STEAM_ONLY=0
NO_SERVICE=0
RESTART_STEAM=1
FORCE=0

say() { printf '==> %s\n' "$*"; }
warn() { printf 'sd-widgets: %s\n' "$*" >&2; }
die() {
  printf 'sd-widgets: %s\n' "$*" >&2
  exit 1
}

usage() {
  cat <<'TXT'
usage: install.sh <deck|daemon> [options]

  --version V         install a specific release (default: the newest)
  --local FILE        use a local AppImage instead of downloading
  --dir DIR           install prefix (default: ~/.local)
  --no-steam          do not touch the Steam library (deck)
  --steam-only        only add the installed app to the Steam library
  --no-steam-restart  never restart Steam; print what to do instead
  --no-service        do not enable the daemon service (daemon)
  --force             write the Steam shortcut even if Steam is running
  -h, --help          this text

Environment: SDW_BASE overrides the release host.
TXT
}

while [ $# -gt 0 ]; do
  case "$1" in
  deck | daemon) APP="$1" ;;
  --version)
    [ $# -ge 2 ] || die "--version needs a value"
    VERSION="${2#v}"
    shift
    ;;
  --local)
    [ $# -ge 2 ] || die "--local needs a file"
    LOCAL_FILE="$2"
    shift
    ;;
  --dir)
    [ $# -ge 2 ] || die "--dir needs a path"
    PREFIX="$2"
    shift
    ;;
  --no-steam) NO_STEAM=1 ;;
  --steam-only) STEAM_ONLY=1 ;;
  --no-steam-restart) RESTART_STEAM=0 ;;
  --no-service) NO_SERVICE=1 ;;
  --force) FORCE=1 ;;
  -h | --help)
    usage
    exit 0
    ;;
  *)
    warn "unknown argument: $1"
    usage >&2
    exit 2
    ;;
  esac
  shift
done

[ -n "$APP" ] || {
  usage >&2
  exit 2
}
command -v curl >/dev/null 2>&1 || die "curl is required"

PREFIX="${PREFIX%/}"
case "$PREFIX" in
/*) ;;
*) PREFIX="$PWD/$PREFIX" ;;
esac
BIN="$PREFIX/bin"
SHARE="$PREFIX/share/sdw-widgets"
APPS="$PREFIX/share/applications"
ICONS="$PREFIX/share/icons/hicolor/512x512/apps"

# Running from a checkout (bash packaging/install.sh ...) lets the script use
# the local artwork and helper instead of downloading them.
HERE=""
SELF="${BASH_SOURCE[0]:-}"
if [ -n "$SELF" ] && [ -f "$SELF" ]; then
  HERE="$(cd "$(dirname "$SELF")" && pwd)"
fi
REPO=""
if [ -n "$HERE" ] && [ -d "$HERE/../assets" ]; then
  REPO="$(cd "$HERE/.." && pwd)"
fi

fetch() { curl -fsSL --retry 3 --connect-timeout 15 --max-time 600 "$1"; }

resolve_version() {
  if [ -n "$VERSION" ]; then
    return
  fi
  local effective
  effective="$(curl -fsIL --retry 2 --connect-timeout 15 --max-time 30 \
    -o /dev/null -w '%{url_effective}' "$BASE/releases/latest" || true)"
  case "$effective" in
  */tag/*) VERSION="${effective##*/tag/}" ;;
  esac
  VERSION="${VERSION#v}"
  [ -n "$VERSION" ] || die "cannot find the newest release on $BASE"
}

sha_check() {
  local file="$1" name="$2" dir="$3"
  if fetch "$BASE/releases/download/v$VERSION/SHA256SUMS" >"$dir/SHA256SUMS" 2>/dev/null; then
    if grep -q " $name\$" "$dir/SHA256SUMS" && command -v sha256sum >/dev/null 2>&1; then
      (cd "$dir" && grep " $name\$" SHA256SUMS >.sum && sha256sum -c .sum >/dev/null) ||
        die "checksum mismatch for $name"
      say "checksum ok"
    fi
  fi
}

download_app() {
  local app="$1" asset="$2" dest="$3"
  if [ -n "$LOCAL_FILE" ]; then
    [ -f "$LOCAL_FILE" ] || die "no such file: $LOCAL_FILE"
    cp "$LOCAL_FILE" "$dest"
    return
  fi
  local tmp
  tmp="$(mktemp -d)"
  say "downloading $asset"
  fetch "$BASE/releases/download/v$VERSION/$asset" >"$tmp/$asset" ||
    die "download failed: $BASE/releases/download/v$VERSION/$asset"
  sha_check "$tmp/$asset" "$asset" "$tmp"
  cp "$tmp/$asset" "$dest"
  rm -rf "$tmp"
}

write_launcher() {
  local app="$1"
  cat >"$BIN/$app" <<LAUNCH
#!/bin/sh
# sd-widgets launcher: run the AppImage directly, or extract it when FUSE is
# missing (some images ship without libfuse2).
SELF="\$(readlink -f "\$0" 2>/dev/null || echo "\$0")"
APP="\$(dirname "\$SELF")/$app.AppImage"
[ -x "\$APP" ] || { echo "sd-widgets: \$APP is missing" >&2; exit 1; }
if [ -e /dev/fuse ] && { command -v fusermount3 >/dev/null 2>&1 || command -v fusermount >/dev/null 2>&1; }; then
  exec "\$APP" "\$@"
fi
exec "\$APP" --appimage-extract-and-run "\$@"
LAUNCH
  chmod 755 "$BIN/$app"
}

install_app() {
  local app="$1" asset="$2"
  mkdir -p "$BIN"
  local tmp="$BIN/.$app.AppImage.new.$$"
  download_app "$app" "$asset" "$tmp"
  chmod 755 "$tmp"
  mv -f "$tmp" "$BIN/$app.AppImage"
  write_launcher "$app"
  say "installed $BIN/$app"
}

install_icon() {
  mkdir -p "$ICONS"
  if [ -n "$REPO" ] && [ -f "$REPO/assets/icons/sd-widgets-512.png" ]; then
    cp "$REPO/assets/icons/sd-widgets-512.png" "$ICONS/sdw-widgets.png"
  else
    fetch "$BASE/raw/tag/v$VERSION/assets/icons/sd-widgets-512.png" \
      >"$ICONS/sdw-widgets.png" 2>/dev/null || rm -f "$ICONS/sdw-widgets.png"
  fi
  [ -f "$ICONS/sdw-widgets.png" ] || warn "icon not downloaded; the menu entry keeps a generic icon"
}

write_desktop_entry() {
  local app="$1" name="$2" comment="$3" keywords="$4" wm="${5:-$1}"
  mkdir -p "$APPS"
  cat >"$APPS/$app.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=$name
GenericName=Steam Deck control surface
Comment=$comment
Exec=$BIN/$app
Icon=sd-widgets
Terminal=false
Categories=Utility;System;RemoteAccess;
Keywords=$keywords
StartupWMClass=$wm
DESKTOP
  chmod 644 "$APPS/$app.desktop"
  command -v update-desktop-database >/dev/null 2>&1 &&
    update-desktop-database "$APPS" >/dev/null 2>&1 || true
}

# ---------------------------------------------------------------- steam

steam_running() {
  pgrep -x steam >/dev/null 2>&1
}

steam_helper() {
  if [ -n "$REPO" ] && [ -f "$REPO/packaging/linux/steam_shortcut.py" ]; then
    printf '%s\n' "$REPO/packaging/linux/steam_shortcut.py"
    return
  fi
  mkdir -p "$SHARE"
  if [ ! -f "$SHARE/steam_shortcut.py" ]; then
    fetch "$BASE/raw/tag/v$VERSION/packaging/linux/steam_shortcut.py" \
      >"$SHARE/steam_shortcut.py" || return 1
  fi
  printf '%s\n' "$SHARE/steam_shortcut.py"
}

steam_art() {
  local dir="$SHARE/art"
  if [ -n "$REPO" ] && [ -d "$REPO/assets/steam" ]; then
    printf '%s\n' "$REPO/assets/steam"
    return
  fi
  local file
  for file in grid.png hero.png logo.png; do
    [ -f "$dir/$file" ] && continue
    mkdir -p "$dir"
    fetch "$BASE/raw/tag/v$VERSION/assets/steam/$file" >"$dir/$file" 2>/dev/null ||
      rm -f "$dir/$file"
  done
  printf '%s\n' "$dir"
}

add_to_steam() {
  if [ "$NO_STEAM" = 1 ]; then
    return 0
  fi
  command -v python3 >/dev/null 2>&1 || {
    warn "python3 is missing; add sd-widgets through Steam → Add a Non-Steam Game"
    return 0
  }
  local helper
  helper="$(steam_helper)" || {
    warn "cannot fetch the Steam helper"
    return 0
  }
  local art
  art="$(steam_art)"
  local icon="$ICONS/sdw-widgets.png"
  [ -f "$icon" ] || icon=""

  local restarted=0
  local force_arg=()
  if [ "$FORCE" = 1 ]; then
    force_arg=(--force)
    if steam_running; then
      warn "Steam is running; the shortcut may be overwritten when Steam exits"
    fi
  elif steam_running; then
    if [ "$RESTART_STEAM" = 0 ] || ! command -v steam >/dev/null 2>&1; then
      warn "Steam is running: close it completely, then run"
      warn "  curl -fsSL $BASE/raw/branch/main/packaging/install.sh | bash -s -- $APP --steam-only"
      return 0
    fi
    say "Steam is running — restarting it so the library edit is not overwritten"
    steam -shutdown >/dev/null 2>&1 || true
    local i
    for i in $(seq 1 30); do
      steam_running || break
      sleep 1
    done
    if steam_running; then
      warn "Steam did not close; add the shortcut later with:"
      warn "  $helper add --exe $BIN/sdw-deck --name sd-widgets --art $art"
      return 0
    fi
    restarted=1
  fi

  local rc=0
  python3 "$helper" add \
    --exe "$BIN/sdw-deck" \
    --name "sd-widgets" \
    --start-dir "$HOME" \
    --icon "$icon" \
    --icon-png "$icon" \
    --art "$art" "${force_arg[@]}" || rc=$?

  if [ "$restarted" = 1 ]; then
    say "starting Steam again"
    setsid steam >/dev/null 2>&1 </dev/null &
  fi

  case "$rc" in
  0) say "sd-widgets is in the Steam library" ;;
  3) warn "Steam was still running; close it and rerun with --steam-only" ;;
  *) warn "could not edit shortcuts.vdf (code $rc); use Steam → Add a Non-Steam Game" ;;
  esac
}

# ---------------------------------------------------------------- daemon

install_service() {
  if [ "$NO_SERVICE" = 1 ]; then
    say "service skipped; start it with: $BIN/sdw-daemon --headless"
    return
  fi
  if command -v systemctl >/dev/null 2>&1 && systemctl --user show-environment >/dev/null 2>&1; then
    local unit="$HOME/.config/systemd/user/sdw-daemon.service"
    mkdir -p "$(dirname "$unit")"
    cat >"$unit" <<UNIT
[Unit]
Description=sd-widgets daemon
Documentation=$BASE
After=graphical-session.target network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart=$BIN/sdw-daemon --headless
Restart=on-failure
RestartSec=2
# Notifications and screen capture need the session bus and the display, so no
# filesystem sandbox here.
Environment=RUST_BACKTRACE=1

[Install]
WantedBy=default.target
UNIT
    systemctl --user daemon-reload
    systemctl --user enable sdw-daemon
    systemctl --user restart sdw-daemon
    say "systemd user service sdw-daemon is enabled and running"
    return
  fi

  # No user systemd: fall back to the XDG autostart folder and start it now.
  local entry="$HOME/.config/autostart/sdw-daemon.desktop"
  mkdir -p "$(dirname "$entry")"
  cat >"$entry" <<AUTOSTART
[Desktop Entry]
Type=Application
Name=sd-widgets daemon
Comment=Virtual gamepads, extra monitor, notifications and audio for the Steam Deck
Exec=$BIN/sdw-daemon --headless
Icon=sd-widgets
Terminal=false
X-GNOME-Autostart-enabled=true
AUTOSTART
  setsid "$BIN/sdw-daemon" --headless >/dev/null 2>&1 </dev/null &
  say "started the daemon and added it to autostart"
}

# ---------------------------------------------------------------- main

ARCH="$(uname -m)"
[ "$ARCH" = "x86_64" ] || die "no build for $ARCH (only x86_64 for now)"

resolve_version
if [ -n "$LOCAL_FILE" ]; then
  LOCAL_BASE="$(basename "$LOCAL_FILE")"
  case "$LOCAL_BASE" in
  sdw-deck-*-x86_64.AppImage) VERSION="${LOCAL_BASE#sdw-deck-}" && VERSION="${VERSION%-x86_64.AppImage}" ;;
  sdw-daemon-*-x86_64.AppImage) VERSION="${LOCAL_BASE#sdw-daemon-}" && VERSION="${VERSION%-x86_64.AppImage}" ;;
  esac
fi
if [ "$STEAM_ONLY" = 0 ]; then
  say "sd-widgets $VERSION for $ARCH"
  install_icon
fi

case "$APP" in
deck)
  if [ "$STEAM_ONLY" = 0 ]; then
    install_app sdw-deck "sdw-deck-$VERSION-x86_64.AppImage"
    write_desktop_entry sdw-deck "sd-widgets" \
      "Gamepad, extra screen, notifications and Home Assistant for the Deck" \
      "steam;deck;gamepad;remote;monitor;" "sd-widgets"
    [ -x "$BIN/sdw-deck" ] || die "no $BIN/sdw-deck to add to Steam"
  fi
  add_to_steam
  if [ "$STEAM_ONLY" = 0 ]; then
    printf '\nReady. Steam → Library → sd-widgets, or the desktop menu entry.\n'
  fi
  ;;
daemon)
  install_app sdw-daemon "sdw-daemon-$VERSION-x86_64.AppImage"
  write_desktop_entry sdw-daemon "sd-widgets daemon" \
    "Virtual gamepads, extra monitor, notifications and audio for the Steam Deck" \
    "steam;deck;gamepad;viiper;monitor;audio;"
  install_service
  ip="$(hostname -I 2>/dev/null | awk '{print $1}')"
  [ -n "$ip" ] || ip="127.0.0.1"
  printf '\nReady. Open http://%s:47830 to finish the setup, then pair the Deck.\n' "$ip"
  printf 'Logs: journalctl --user -u sdw-daemon -f\n'
  ;;
esac
