# sd-widgets

A **Steam Deck control surface** plus a **PC companion daemon**. It turns the Deck
into a second screen, a virtual gamepad, a notification panel and a Home Assistant
remote — and it looks like a hard-edged, 32 px grid system instead of a phone app.

- **One repo, two binaries**: `sdw-deck` (runs on the Deck) and `sdw-daemon` (runs on the computer).
- **Grid** visual language: no rounding anywhere, hairline borders, 32 px cells,
  and a minimal set of **Material Symbols** icons.
- **Full customisation**: CSS themes, palette pickers, density, tile grid, page and
  tile visibility, interface sounds, per-event sound files.
- **Bilingual**: English and Russian, dark and light.

> The product name is lowercase: `sd-widgets`.

## What it does

### On the Steam Deck (`sdw-deck`)

| Page | What it is |
| --- | --- |
| **Home** | A grid of large touch tiles: gamepad, screen, notifications, Home Assistant, binds, computers. |
| **Computers** | LAN discovery, pairing by password, switching between several computers. |
| **Gamepad** | A drawn Steam Deck with every control, live highlighting, and a bind editor for any control or gesture. |
| **Screen** | The extra monitor stream: start/stop, backend, position, resolution, fps, quality, input forwarding. |
| **Notifications** | Mirrored desktop notifications, newest first, unread counters. |
| **Home Assistant** | An app-style entity browser with search and one-tap actions. |
| **Binds** | Buttons, chords and swipe gestures mapped to actions or remote commands. |
| **Look** | Presets, palette, CSS editor, grid, density, dock/arrows, page visibility, sounds. |
| **Settings** | Device name, language, theme, UI scale, input devices, updates. |

Every page can be reached by the **visible left/right arrows**, by the floating
glyph dock, by three-finger swipes, or by keyboard. Page swaps animate with a
fade, a small rise and a directional slide; tiles stagger in behind it.

### On the computer (`sdw-daemon`)

- **Virtual gamepads via VIIPER** — Xbox 360, DualShock 4, DualSense and generic
  pads, hot-plugged over USBIP. VIIPER can be installed from the GUI.
- **Extra monitor** — creates a real additional monitor and streams it to the Deck:
  - Hyprland: headless output (`hyprctl output create headless`).
  - niri: VKMS output + `niri msg output` (niri has no native virtual outputs).
  - KDE: `krfb-virtualmonitor` with a built-in RFB client.
  - X11 and Windows: `ffmpeg` desktop capture.
  - macOS: `screencapture`.
  - Fallback: mirror an existing output with `grim`/`spectacle`.
- **Automatic layout** — choose right/left/above/below/auto in the daemon and the
  layout is applied live and written into the compositor config
  (`~/.config/niri/config.kdl`, `~/.config/hypr/hyprland.conf`,
  `~/.config/sway/config`, KDE screen config, X11 `xrandr`).
- **Notification mirroring** — watches the desktop notification bus and serves the
  stream to the Deck.
- **Home Assistant** — URL + long-lived token, entities proxied through the daemon
  so the token never lives on the Deck.
- **Setup wizard** — password inputs, dependency detection, install buttons for
  VIIPER and friends, and an explicit line telling you what to add to your
  compositor config when auto-patching is not possible.

## Requirements

**Deck side**

- Linux (SteamOS, CachyOS handheld, any distro), Wayland or X11.
- `input` group membership for raw device reads (optional; gestures fall back to
  touch events).
- Build: Rust 1.88+.

**Computer side**

- Linux, Windows or macOS.
- Optional at runtime, depending on features:
  - VIIPER 0.6+ for virtual gamepads (the daemon can install it).
  - `usbip` for VIIPER local attach.
  - `hyprctl` / `niri` / `krfb-virtualmonitor` / `ffmpeg` / `grim` / `spectacle` /
    `xrandr`, depending on the monitor backend.
  - `pw-play`, `paplay`, `aplay`, `ffplay` or `mpv` on the Deck for interface sounds.

## Install

### Prebuilt

Grab the artifacts from the releases page:

- `sdw-deck-<version>-x86_64.AppImage` — Steam Deck / Linux.
- `sdw-daemon-<version>-windows-x86_64.zip` — Windows.
- `sdw-daemon-<version>-macos-<arch>.dmg` — macOS.

### From source

```bash
git clone https://git.entitybtw.ru/entitybtw/sd-widgets
cd sd-widgets
cargo build --release -p sdw-deck -p sdw-daemon
```

Binaries land in `target/release/`.

## Pairing

1. Start `sdw-daemon` on the computer. The first launch opens the setup wizard and
   shows a **pairing password**.
2. Start `sdw-deck` on the Deck, open **Computers → add computer**, pick the daemon
   from LAN discovery or type its address.
3. Enter the password once. The daemon returns an API token; the Deck stores it per
   host and never needs the password again.
4. Switch computers at any time from the header or the Computers page.

The daemon listens on `47830/tcp` (REST), `47831/udp` (gamepad input) and
`47832/udp` (LAN discovery). All REST calls require the bearer token.

## Extra monitor

The daemon creates a monitor that really exists for the operating system, then
streams it to the Deck at 1280×800 by default.

- Choose **position** in the daemon: auto, right, left, above, below.
- Choose the backend, size, fps and JPEG quality.
- The daemon applies the layout immediately and persists it into your compositor
  config.
- If a compositor cannot be patched automatically, the wizard prints the exact line
  to add.

Details and per-compositor notes: [`docs/MONITOR.md`](docs/MONITOR.md).

## Themes, layout and sounds

The **Look** page is the whole design system:

- five presets: Grid, Monochrome, Nord, Gruvbox, Terminal;
- colour pickers for both palettes (`--bg`, `--card`, `--ink`, `--muted`, `--line`,
  `--accent` and their `--light-*` twins);
- a live CSS editor — any `--name: value;` declaration is picked up;
- grid on/off, cell size and line opacity;
- density, font scale, tile height and column count;
- arrows, dock, labels, glyphs and animations;
- which pages appear in the dock and which tiles appear on Home;
- three synthesized sound packs (`grid`, `soft`, `chip`), volume, and a custom
  WAV path for every event.

The CSS variables and the full theme model are documented in
[`docs/THEMES.md`](docs/THEMES.md).

## API

The Deck and the daemon speak JSON over HTTP. See [`docs/API.md`](docs/API.md) for
the endpoints: pairing, status, gamepad, notifications, Home Assistant, monitor,
remote commands and discovery.

Remote PC commands are a **fixed whitelist** (`suspend`, `lock`, `volume`, media
keys, opening URLs). There is no arbitrary shell endpoint.

## Repository layout

```
crates/sdw-core      shared model, REST client, auth, discovery, UDP, i18n
crates/sdw-deck      the Deck app (egui)
crates/sdw-daemon    the PC daemon, server and GUI (egui)
vendor/viiper-client vendored MIT Rust client for VIIPER
assets               fonts and icons
packaging            desktop files, systemd unit, AppImage / Windows / macOS scripts
docs                 architecture, API, themes, monitor notes
```

## Security

- Passwords are stored as SHA-256 hashes with a random salt; the derived API token
  is random per daemon and per pairing.
- The REST server binds to the LAN and requires the token on every route.
- The extra monitor only ever exposes the monitor the daemon created (or the
  selected output in mirror mode).
- Nothing is executed from untrusted input: remote actions are an enum.

## License

MIT for this repository. The vendored VIIPER **client** is MIT. Material Symbols
icons and the Archivo / JetBrains Mono fonts keep their own licenses (Apache-2.0
and OFL), shipped in `assets/`. The VIIPER
**server** is a separate GPL-3.0 project and is **not** bundled; the daemon can
install it from its own release artifacts if you ask it to.
