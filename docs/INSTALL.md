# Installing sd-widgets

One command installs either side. The script looks at the machine itself: on a
Steam Deck it installs the Deck app, everywhere else the daemon. It is a plain
shell script from this repository; read
[`packaging/install.sh`](../packaging/install.sh) before piping it into `bash`
if you prefer.

```bash
curl -fsSL https://git.entitybtw.ru/entitybtw/sd-widgets/raw/branch/main/packaging/install.sh | bash
```

To choose the role yourself, pass `deck` or `daemon` after `bash -s --`:

```bash
curl -fsSL https://git.entitybtw.ru/entitybtw/sd-widgets/raw/branch/main/packaging/install.sh | bash -s -- daemon
```

The installer always downloads the **newest published release** from
`git.entitybtw.ru`, verifies it against `SHA256SUMS` when that asset exists, and
installs into `~/.local` (no root, no package manager).

The in-app updater takes its source from the settings page instead: an empty
field keeps `git.entitybtw.ru`, and a GitHub repository or a Forgejo/Gitea
instance can be written as its web address. The installer itself stays on the
built-in source, since it is fetched before any setting exists.

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
`systemctl --user enable` plus a `restart`, so the freshly installed build is
the one running. When user systemd is unavailable it
falls back to `~/.config/autostart/sdw-daemon.desktop` and starts the daemon
once. The daemon opens `http://<computer>:47830` for the first-run setup; the
console prints the address with your LAN IP.

The daemon also has a **start with the system** switch in its settings. It
writes a **systemd user unit** (`~/.config/systemd/user/sdw-daemon.service`,
`WantedBy=graphical-session.target`, enabled but not started — the daemon is
already running), the XDG autostart entry as the fallback for a machine with no
user systemd, and, because niri and Hyprland ignore that folder, one line in
their own config (`spawn-at-startup` or `exec-once`). Turning the switch off
removes all three again; when user systemd disappears later, the unit is
removed on the next start and the desktop entry keeps working. The command
carries `--hidden`, so at login the daemon sits in the tray instead of opening
a window; the tray, `sdw-daemon --show` and starting the daemon again bring the
window back. Debug builds under `target/` never write themselves into the
session config.

### The Deck and the Steam library

`install.sh deck` adds sd-widgets as a **non-Steam shortcut** by writing one
entry into `~/.steam/steam/userdata/<id>/config/shortcuts.vdf`, with a backup
next to it, and copies the artwork into `config/grid/` (`<appid>.png`,
`_hero.png`, `_logo.png`, `_icon.png`). The entry launches the wrapper above, so
Game Mode starts the same build as the desktop menu.

Every write keeps the previous file as `shortcuts.vdf.bak-<stamp>` and checks
that each existing shortcut survives the rewrite before it replaces anything;
the helper also reads its own output back, so a bad parse can no longer shorten
the library. If the list ever comes back short, close Steam completely and run
`python3 steam_shortcut.py restore`: it puts the backup with the most shortcuts
back and keeps the current file as `shortcuts.vdf.before-restore-<stamp>`.

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

### The Steam Input layout

A non-Steam game gets a default Steam Input layout ("Gamepad With Joystick
Trackpad"), and that layout turns both Deck touchpads into mouse/joystick
sources. sd-widgets reads the touchpads from the kernel itself, so the default
layout only interferes: a cursor moves while a gesture is drawn on a pad, and a
pad click arrives as a mouse click on top of the real one.

The installer also writes a Neptune (Steam Deck) layout named **sd-widgets** into
`<steam root>/steamapps/common/Steam Controller Configs/<account>/config/sd-widgets/`,
selects it for the shortcut in `configset_controller_neptune.vdf`, and backs up
both files once as `*.sdw-backup`. The layout has one action set, **Main**:
buttons, sticks, triggers and the d-pad stay on xinput, while **both touchpads
are left unbound** because they are the app's own input. With no pad bound,
Steam has nothing to turn into a mouse, so a pad touch reaches only the app and
pad binds and pad gestures see one source. In Game Mode Steam Input keeps the
Deck's raw HID node open and the kernel removes the evdev node; the app reads
report 9 from `/dev/hidraw*` for as long as Steam Input's virtual pad is
present, and goes back to evdev when Steam quits.

Steam must be closed while its controller files are written, so the installer
uses the same shutdown window as the shortcut. If the profile has no Steam
Controller Configs folder yet, the script says so: launch Steam once, then rerun
`... | bash -s -- deck --steam-only`. `--no-layout` skips this step. To go back,
restore `controller_neptune.vdf` from its `*.sdw-backup` or choose another layout
in **Steam → sd-widgets → Properties → Controller**.

## Options

| Option | Meaning |
| --- | --- |
| `--version V` | install release `V` instead of the newest |
| `--local FILE` | install an AppImage you already have (version is taken from its name) |
| `--dir DIR` | prefix instead of `~/.local` |
| `--no-service` | do not install or start the daemon service |
| `--no-steam` | do not touch the Steam library |
| `--no-layout` | do not write the Steam Input layout (deck) |
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
in place; nothing else changes. The card then offers a **restart** button: the
daemon asks its systemd user service to come back, so an update never needs a
terminal. To update the launcher and the Steam entry,
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
| Non-Steam games disappeared from the library | Close Steam completely, then run `python3 steam_shortcut.py restore` (from the installer folder, with the same `HOME`): it restores the `shortcuts.vdf.bak-*` copy with the most shortcuts. Start Steam afterwards. |
| Touchpads move a cursor in Game Mode | The `sd-widgets` layout is not selected. Rerun with `--steam-only` and check **Steam → sd-widgets → Properties → Controller**; the action set is **Main** and both pads are unbound on purpose. |
| Touchpads are dead in Game Mode | Steam Input holds the Deck controller and hid-steam has removed its evdev node, so the app reads the raw HID report from `/dev/hidraw*`. Check that the user may read it: `ls -l /dev/hidraw*`. If the nodes are `root:root 0600`, add the user to the `input` group (`sudo usermod -aG input $USER`) and log in again; the input page names the failure when the node cannot be opened. |
| `systemctl --user` not available | The installer used the XDG autostart entry. Start it now with `~/.local/bin/sdw-daemon --headless &`. |
| The daemon says `pactl`/`parec` missing | Install `pipewire-pulse` or `pulseaudio-utils`; audio is documented in [`AUDIO.md`](AUDIO.md). |
| No `python3` on the Deck | SteamOS ships it. Without it the app is installed but the Steam entry must be added via **Steam → Add a Non-Steam Game**. |
