# Packaging

| Path | Produces |
| --- | --- |
| `install.sh` | The one-command installer for the Deck and the daemon (downloads a release, writes launchers, services and the Steam shortcut). |
| `linux/` | `.desktop` entries, a systemd user unit, an autostart entry, `build-appimage.sh`, the Steam helper and artwork generator. |
| `windows/` | `build.ps1` and a short readme for the zip. |

All build scripts write to `dist/` in the repository root.

## One-command install

```bash
curl -fsSL https://git.entitybtw.ru/entitybtw/sd-widgets/raw/branch/main/packaging/install.sh | bash -s -- deck
curl -fsSL https://git.entitybtw.ru/entitybtw/sd-widgets/raw/branch/main/packaging/install.sh | bash -s -- daemon
```

`--local dist/sdw-deck-0.6.1-x86_64.AppImage` installs a build from this checkout
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

`shortcuts.vdf` is binary VDF and holds the whole non-Steam library, so
`steam_shortcut.py` reads it recursively (an entry nests a `tags` map, which a
flat reader would stop at) and refuses to write unless every other entry comes
back unchanged from its own output. Each write leaves
`shortcuts.vdf.bak-<stamp>`; `steam_shortcut.py restore` puts the backup with
the most shortcuts back and keeps the current file as
`shortcuts.vdf.before-restore-<stamp>`.

`packaging/linux/steam_layout.py` installs
`packaging/linux/controller_neptune_sdw.vdf` as the Deck layout of that shortcut
and selects it in `configset_controller_neptune.vdf`. The layout keeps a normal
xinput pad but leaves both touchpads unbound: sd-widgets reads them itself.
Existing files are backed up once as `*.sdw-backup`; `--no-layout` skips the
step.

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

The release format is a **portable zip** (`dist/sdw-daemon-<version>-windows-x86_64.zip`):
unpack it anywhere and run `sdw-daemon.exe`. There is no MSI and no service;
the wizard handles setup, and "start with Windows" is a checkbox in the GUI.

`build.ps1` also builds a graphical installer when Inno Setup 6 (`iscc`) is on
the machine, from `packaging/windows/installer.iss`; without `iscc` that step is
skipped. The daemon itself needs no Administrator rights — the Virtual Display
Driver and usbipd ask for elevation only when installed. Both the driver and
`ffmpeg` can be installed in one click from the daemon's own install page.
