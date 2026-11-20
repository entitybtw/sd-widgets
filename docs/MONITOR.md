# Extra monitor

The daemon creates an additional monitor on the computer, positions it, and streams
it to the Deck. Two modes:

- **Virtual** — a new monitor the compositor does not otherwise have.
- **Mirror** — copy an existing output (fallback when virtual outputs are not
  available).

Default target: **1280×800 @ 60 Hz**, 15 fps, JPEG quality 75.

## Backends

### Hyprland (virtual)

```
hyprctl output create headless sdw-monitor
```

The daemon then applies the requested position and size, and writes a matching
`monitor =` line into `~/.config/hypr/hyprland.conf`.

### niri / biri (virtual, native)

Stock niri cannot create outputs. **biri**, a niri fork that carries the
virtual-output work, can:

```
niri msg create-virtual-output --name sdw-deck --width 1280 --height 800 --refresh-rate 60
niri msg output sdw-deck custom-mode "1280x800@60"
niri msg remove-virtual-output sdw-deck
```

The daemon detects this at runtime by looking for `create-virtual-output` in
`niri msg --help`, so one binary serves both stock niri and biri. When the command
exists, the extra screen is a real output drawn by the compositor itself: no VKMS
module, no second DRM device, and it works with the NVIDIA proprietary driver.
The wizard lists `biri-altinstall-git` (AUR, `paru -S biri-altinstall-git`) among
the tools, and the session has to be biri rather than niri.

The output is named `sdw-deck`, laid out on the requested side and removed again
when the stream stops. Without biri the daemon falls back to the VKMS path below.

### niri on stock (virtual, via VKMS)

Stock niri has no native virtual outputs (see biri above). The daemon:

1. loads `vkms`, trying `modprobe`, passwordless `sudo`, `pkexec` and finally the
   password typed in the wizard, in that order,
2. polls `/sys/class/drm` for the new connector (a virtual machine can take a few
   seconds),
3. calls `niri msg output <name> …` to enable, size and position it,
4. writes the output block into `~/.config/niri/config.kdl`.

The wizard shows a **readiness** row (module in the kernel, module loaded, output
found, config path). The **prepare vkms** button loads the module *and* writes
`/etc/modules-load.d/sd-widgets.conf` and
`/etc/modprobe.d/sd-widgets.conf`, so the extra output survives a reboot without
manual files.

If `vkms` cannot be loaded, or niri refuses the connector, the daemon falls back
to mirroring an existing niri output with `grim` and says why in the status note.

niri draws every output with one render GPU. A VKMS connector is a separate DRM
device, and with the NVIDIA proprietary driver niri cannot produce a buffer the
VKMS scanner accepts (`No supported renderer buffer format found`), so the
connector stays off. On such a machine the extra-screen button gives a mirror,
which is honest and works. A true second screen on niri needs biri, whose virtual
output is drawn by niri itself (see above).

The daemon positions a new output against the outermost edge of the existing
desktop, not against the primary display, so a "right" screen does not land on
top of a second real monitor. Stopping the stream turns the virtual output off
again and restores the compositor config that was there before the layout was
written.

### KDE (virtual)

Uses `krfb-virtualmonitor` to expose a virtual screen and the built-in RFB client
to capture it. The position is applied through the KDE screen configuration
(kscreen) when available.

### X11 (virtual)

A virtual X screen sized to the request is captured with `ffmpeg -f x11grab`. The
layout is applied with `xrandr`.

### Windows (virtual/mirror)

Desktop capture with `ffmpeg -f gdigrab`. The extra monitor is presented as a
window on the chosen side; true virtual displays depend on the display driver.

## Virtual machines

A guest usually has one virtual GPU and no spare connector. The daemon detects
the virtualisation (`systemd-detect-virt`, or the hypervisor flag plus the DMI
product name) and reports it in the monitor status as `vm`. The automatic backend
then goes through VKMS for niri, sway, GNOME and an unidentified Wayland session,
because that path does not depend on the host GPU.

Nothing needs to be prepared by hand: press **prepare vkms** once in the wizard
(the sudo password is optional if passwordless `sudo` or `pkexec` already works),
then start the screen. The readiness row turns green as the module, the connector
and the config are found.

## Position

`MonitorPosition` is one of `auto`, `right`, `left`, `above`, `below`.

- `auto` places the new monitor to the right of the primary output.
- The choice is applied live and persisted in the compositor config.
- The Deck can change the position from the Screen page; the daemon restarts the
  capture with the new layout.

## Configuration hints

When automatic patching is not possible (read-only config, unrecognised compositor,
Windows), the setup wizard and the daemon GUI show the exact snippet to add.
Examples:

niri (`~/.config/niri/config.kdl`):

```kdl
output "Virtual-1" {
    mode "1280x800@60.000"
    position x=2560 y=0
    scale 1.0
}
```

Hyprland (`~/.config/hypr/hyprland.conf`):

```
monitor = Virtual-1, 1280x800@60, 2560x0, 1
```

sway (`~/.config/sway/config`):

```
output Virtual-1 resolution 1280x800 position 2560 0
```

X11:

```bash
xrandr --newmode "1280x800_60" …
xrandr --addmode VIRTUAL1 1280x800_60
xrandr --output VIRTUAL1 --mode 1280x800_60 --right-of eDP-1
```

## Requirements per backend

| Backend | Needs |
| --- | --- |
| Hyprland | `hyprctl` |
| niri | `niri`, `vkms` module (one-button prepare), write access to the niri config |
| KDE | `krfb-virtualmonitor`, RFB support |
| X11 | `ffmpeg`, `xrandr` |
| Windows | `ffmpeg` |
| mirror | `grim` (Wayland) or `spectacle` (KDE) |

The daemon reports which backend it selected and why in its log and in the
`monitor` block of `/api/v1/status`. That block also carries `vm`, `vkms` and
`vkms_loaded`, which is exactly what the readiness row shows.
