# Installing sd-widgets

Two one-line commands: one for the Steam Deck app, one for the computer daemon.
Both are plain shell scripts from this repository; read
[`packaging/install.sh`](../packaging/install.sh) before piping it into `bash`
if you prefer.

```bash
# Steam Deck (Desktop Mode → Konsole)
curl -fsSL https://git.entitybtw.ru/entitybtw/sd-widgets/raw/branch/main/packaging/install.sh | bash -s -- deck

# computer
curl -fsSL https://git.entitybtw.ru/entitybtw/sd-widgets/raw/branch/main/packaging/install.sh | bash -s -- daemon
```

The installer always downloads the **newest published release** from
`git.entitybtw.ru`, verifies it against `SHA256SUMS` when that asset exists, and
installs into `~/.local` (no root, no package manager).

## What it puts on disk

| Path | What |
| --- | --- |
| `~/.local/bin/sdw-deck` | launcher for the Deck app |
| `~/.local/bin/sdw-deck.AppImage` | the real build; updates replace this file |
| `~/.local/bin/sdw-daemon` | launcher for the daemon |
| `~/.local/bin/sdw-daemon.AppImage` | the real daemon build |
| `~/.local/share/applications/*.desktop` | desktop menu entries |
| `~/.local/share/icons/hicolor/512x512/apps/sdw-widgets.png` | icon |
| `~/.config/systemd/user/sdw-daemon.service` | daemon unit (daemon install) |

The launcher is a three-line wrapper that starts the AppImage directly, or with
`--appimage-extract-and-run` when the system has no `libfuse2`. Because the
wrapper runs the real file, the in-app updater still replaces the AppImage and
the wrapper survives.

### The daemon

`install.sh daemon` writes a **systemd user unit** and runs
`systemctl --user enable --now sdw-daemon`. When user systemd is unavailable it
falls back to `~/.config/autostart/sdw-daemon.desktop` and starts the daemon
once. The daemon opens `http://<computer>:47830` for the first-run setup; the
console prints the address with your LAN IP.

The daemon also has a **start with the system** switch in its settings. It
writes the XDG autostart entry and, because niri and Hyprland ignore that
folder, one line in their own config (`spawn-at-startup` or `exec-once`).
Turning the switch off removes both again. The command carries `--hidden`, so
at login the daemon sits in the tray instead of opening a window; the tray,
`sdw-daemon --show` and starting the daemon again bring the window back.
Debug builds under `target/` never write themselves into the session config.

### The Deck and the Steam library

`install.sh deck` adds sd-widgets as a **non-Steam shortcut** by writing one
entry into `~/.steam/steam/userdata/<id>/config/shortcuts.vdf`, with a backup
next to it, and copies the artwork into `config/grid/` (`<appid>.png`,
`_hero.png`, `_logo.png`, `_icon.png`). The entry launches the wrapper above, so
Game Mode starts the same build as the desktop menu.

Steam keeps `shortcuts.vdf` in memory and rewrites it when it exits. The script
therefore checks for a running Steam:

- if Steam runs and `steam` is on `PATH`, it asks Steam to shut down, waits for
  the process to leave, writes the shortcut and starts Steam again;
- if Steam does not close in time, it prints the exact command to repeat later
  (`... | bash -s -- deck --steam-only`);
- `--no-steam-restart` skips the restart and prints the instructions instead;
- `--force` writes while Steam runs (the entry may be lost when Steam exits).

Icons, artwork and the helper come from the same release tag, so a pinned
`--version` installs matching files.

## Options

| Option | Meaning |
| --- | --- |
| `--version V` | install release `V` instead of the newest |
| `--local FILE` | install an AppImage you already have (version is taken from its name) |
| `--dir DIR` | prefix instead of `~/.local` |
| `--no-service` | do not install or start the daemon service |
| `--no-steam` | do not touch the Steam library |
| `--steam-only` | only add the already installed app to Steam |
| `--no-steam-restart` | never restart Steam automatically |
| `--force` | write the Steam shortcut even while Steam runs |

## After installing

1. **Computer**: open `http://<computer>:47830`, set the pairing password and
   enable what you need (gamepads, monitor, notifications, audio).
2. **Deck**: open **Computers**, pick the machine from discovery, enter the
   password once. The token stays on the Deck.
3. **Terminal**: the menu and Steam use absolute paths; if you want
   `sdw-deck`/`sdw-daemon` in a shell, add `~/.local/bin` to `PATH`.
4. **Game Mode**: the library entry is called **sd-widgets**. The first launch
   from Game Mode works like any non-Steam game; `View` opens Settings and `Y`
   opens Home inside the app.

## Updating and removing

The apps update themselves (**Settings → Updates** on the Deck, the same card in
the daemon window, or `sdw-daemon --download-update`). The AppImage is replaced
in place; nothing else changes. To update the launcher and the Steam entry,
rerun the install command — it is idempotent and updates the existing shortcut
instead of adding a second one.

Removing by hand:

```bash
systemctl --user disable --now sdw-daemon 2>/dev/null
rm -f ~/.config/systemd/user/sdw-daemon.service \
      ~/.local/bin/sdw-deck ~/.local/bin/sdw-deck.AppImage \
      ~/.local/bin/sdw-daemon ~/.local/bin/sdw-daemon.AppImage \
      ~/.local/share/applications/sdw-deck.desktop \
      ~/.local/share/applications/sdw-daemon.desktop
```

The Steam shortcut is removed in **Steam → right click → Manage → Remove
non-Steam game**. Configuration and addons stay in
`~/.config/sd-widgets/` until you delete them.

## Troubleshooting

| Symptom | Fix |
| --- | --- |
| `AppImage` does not start, `libfuse` error | The wrapper falls back to extraction; if that also fails, install `libfuse2` (Debian/Ubuntu) or `fuse2` (Arch). |
| The library entry is missing | Steam was running or restarted too late. Close Steam, then `... | bash -s -- deck --steam-only`. |
| Wrong cover in the library | Steam names artwork by app id. Rerun the installer with `--steam-only`; the artwork is copied for both id variants. |
| `systemctl --user` not available | The installer used the XDG autostart entry. Start it now with `~/.local/bin/sdw-daemon --headless &`. |
| The daemon says `pactl`/`parec` missing | Install `pipewire-pulse` or `pulseaudio-utils`; audio is documented in [`AUDIO.md`](AUDIO.md). |
| No `python3` on the Deck | SteamOS ships it. Without it the app is installed but the Steam entry must be added via **Steam → Add a Non-Steam Game**. |
