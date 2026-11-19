# Packaging

| Path | Produces |
| --- | --- |
| `install.sh` | The one-command installer for the Deck and the daemon (downloads a release, writes launchers, services and the Steam shortcut). |
| `linux/` | `.desktop` entries, a systemd user unit, an autostart entry, `build-appimage.sh`, the Steam helper and artwork generator. |
| `windows/` | `build.ps1` and a short readme for the zip. |
| `macos/` | `build-dmg.sh`, which builds a `.app` and a DMG per architecture. |

All build scripts write to `dist/` in the repository root.

## One-command install

```bash
curl -fsSL https://git.entitybtw.ru/entitybtw/sd-widgets/raw/branch/main/packaging/install.sh | bash -s -- deck
curl -fsSL https://git.entitybtw.ru/entitybtw/sd-widgets/raw/branch/main/packaging/install.sh | bash -s -- daemon
```

`--local dist/sdw-deck-0.3.1-x86_64.AppImage` installs a build from this checkout
without downloading. The full list of options is in
[`docs/INSTALL.md`](../docs/INSTALL.md).

## Linux

```bash
packaging/linux/build-appimage.sh all      # sdw-deck + sdw-daemon
packaging/linux/build-appimage.sh deck
```

`appimagetool` is taken from `PATH` or downloaded to
`$XDG_CACHE_HOME/sdw-widgets/tools`.

## Steam artwork and shortcuts

`packaging/linux/make-steam-art.py` redraws `assets/steam/{grid,hero,logo}.png`
(Pillow is required). `packaging/linux/steam_shortcut.py` writes the non-Steam
shortcut and copies that artwork into Steam's `config/grid/`; `install.sh`
calls it and handles closing/restarting Steam.

Install the user unit:

```bash
install -Dm644 packaging/linux/sdw-daemon.service ~/.config/systemd/user/sdw-daemon.service
systemctl --user enable --now sdw-daemon
```

Or autostart the daemon with the session:

```bash
install -Dm644 packaging/linux/sdw-daemon-autostart.desktop ~/.config/autostart/sdw-daemon.desktop
```

## Windows

```powershell
pwsh -File packaging/windows/build.ps1
```

## macOS

```bash
packaging/macos/build-dmg.sh arm64
packaging/macos/build-dmg.sh x86_64
```
