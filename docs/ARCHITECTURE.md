# Architecture

```
┌──────────────────────────┐        HTTP/JSON :47830       ┌──────────────────────────┐
│  sdw-deck (Steam Deck)   │ ────────────────────────────▶ │  sdw-daemon (computer)   │
│  egui, evdev, sounds     │ ◀──────────────────────────── │  axum-free tiny_http,    │
│                          │        UDP :47831 input       │  VIIPER, capture, HA     │
└──────────────────────────┘ ◀──────────────────────────── └──────────────────────────┘
             │                                                          │
             └──────────── UDP :47832 discovery ────────────────────────┘
```

## Crates

### `sdw-core` (shared)

- `model.rs` — every serialized type: `DaemonStatus`, `DeckConfig`, `ThemeCfg`,
  `SoundCfg`, `MonitorStatus`, `MonitorStart`, notifications, Home Assistant,
  binds, `VERSION`. This is the contract between the two binaries.
- `api.rs` — blocking `ureq` client used by the Deck.
- `auth.rs` — password hashing, token generation and constant-time comparison.
- `discovery.rs` — UDP LAN discovery.
- `udp.rs` — gamepad input packets and `GamepadState`.
- `config.rs` — Deck config load/save (atomic write).
- `i18n.rs` — `t!(lang, "ru", "en")`.
- `ha.rs` — Home Assistant helpers.
- `addons.rs` — declarative addon manifests, install/remove, settings values,
  `AddonInfo` for the status payload.
- `util.rs` — paths, process helpers, addresses, platform detection.

### `sdw-deck` (the Deck)

- `app.rs` — state, polling, pairing, page routing, chrome (top bar, arrows, dock,
  page transition), key input.
- `pages.rs` — home grid and every page, including the Look page.
- `clock.rs` — the four clock renderers and their controls.
- `addons_ui.rs` — the Addons page: import, enable, settings, themes, clocks and
  daemon action buttons.
- `theme.rs` — Grid tokens, CSS variable parser, presets, backdrop, tiles,
  arrows, switches, chips.
- `deck_view.rs` — the drawn Steam Deck and live control highlighting.
- `binds.rs` — bind list, editor and capture.
- `input.rs` — evdev reader, gestures and button codes.
- `sound.rs` — synthesized WAV packs and playback.
- `sys.rs` — the Deck's own output volume and panel brightness (`wpctl`/`pactl`,
  sysfs/`brightnessctl`), used by the pull-down panel.
- `main.rs` — CLI and eframe entry.

### `sdw-daemon` (the computer)

- `server.rs` — `tiny_http` router, auth middleware, all REST handlers.
- `state.rs` — shared state, logger, job manager, Home Assistant runtime.
- `viiper.rs` — VIIPER client, pad lifecycle, password handling.
- `monitor.rs` — monitor creation per backend, layout, config patching, capture,
  RFB client, frame buffer.
- `notifications.rs` — desktop notification watcher.
- `audio.rs` — `parec` for the default monitor, one `pw-record` per picked
  application with a Rust mixer (PulseAudio falls back to a null sink), a
  bounded PCM ring buffer and the blocking `Read` used by the HTTP stream.
- `server.rs` also routes `addon:<id>:<action>` through the declared action
  runner; disabled addons and undeclared actions are rejected.
- `install.rs` — dependency detection and install jobs.
- `gui/` — hoplite-style hard-edge egui setup wizard and control panel.
- `config.rs` — daemon config in TOML.

### `vendor/viiper-client`

A vendored copy of the MIT Rust client for the VIIPER server. The server itself is
GPL-3.0 and is installed separately; it is not linked or bundled.

## Addons

`sdw-core::addons` owns the format: a folder with `addon.json`, a theme CSS, a
clock JSON, tiles, sounds and daemon actions. Both binaries read the same folder
in `~/.config/sd-widgets/addons`; each keeps its own enabled list and values in
its own config. The Deck applies a theme as the preset `addon:<id>`, adds tiles
to Home and clock presets to the Clock page. The daemon lists the packages in
`DaemonStatus.addons` and runs an action only when the package is enabled. The
format is documented in [`ADDONS.md`](ADDONS.md).

## Threads

The daemon is deliberately simple:

- one thread for the HTTP server,
- one for the UDP input port,
- one per running capture,
- one for notification watching,
- one for each install job,
- the GUI on the main thread.

Shared state uses `Arc<AppState>` with atomics and mutexes around the pieces that
truly need it (monitor, VIIPER, jobs, notifications).

## Auth

1. The daemon generates a random API token at first setup.
2. The pairing password is stored as SHA-256 with a random salt.
3. `POST /api/v1/pair` compares the password in constant time and returns the
   token.
4. The Deck stores `{ host, token }` in `DeckConfig.hosts`.
5. Every authenticated route compares the `Authorization: Bearer` token.

## Design system

- 32 px grid, `rgba(128,158,197,0.09)` hairlines.
- No rounded corners anywhere (`radius` is forced to 0 in the theme engine).
- Flat surfaces, 1 px borders, a 2 px accent cap on selected tiles.
- Motion: cubic ease, page fade + rise + directional slide, staggered tiles.
- Typography: Archivo for UI, JetBrains Mono for labels and data.

## Failure modes

- Daemon unreachable → Deck keeps working locally and shows `offline`; tiles that
  need the PC are dimmed.
- VIIPER missing → gamepad tab explains what to install and links the install job.
- Monitor backend unsupported → mirror fallback, with the exact config line shown.
- No notification daemon → notifications page is empty but healthy.
