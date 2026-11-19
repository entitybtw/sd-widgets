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

### niri (virtual, via VKMS)

niri has no native virtual outputs. The daemon:

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

If `vkms` cannot be loaded, the daemon falls back to mirroring an existing niri
output with `grim`.

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

### macOS (mirror)

`screencapture` of the selected display. macOS does not allow creating virtual
displays without a kernel extension.

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
Windows/macOS), the setup wizard and the daemon GUI show the exact snippet to add.
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
| macOS | `screencapture` |
| mirror | `grim` (Wayland) or `spectacle` (KDE) |

The daemon reports which backend it selected and why in its log and in the
`monitor` block of `/api/v1/status`. That block also carries `vm`, `vkms` and
`vkms_loaded`, which is exactly what the readiness row shows.
