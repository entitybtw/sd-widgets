# Packaging

| Folder | Produces |
| --- | --- |
| `linux/` | `.desktop` entries, a systemd user unit, an autostart entry and `build-appimage.sh`. |
| `windows/` | `build.ps1` and a short readme for the zip. |
| `macos/` | `build-dmg.sh`, which builds a `.app` and a DMG per architecture. |

All scripts write to `dist/` in the repository root.

## Linux

```bash
packaging/linux/build-appimage.sh all      # sdw-deck + sdw-daemon
packaging/linux/build-appimage.sh deck
```

`appimagetool` is taken from `PATH` or downloaded to
`$XDG_CACHE_HOME/sdw-widgets/tools`.

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
