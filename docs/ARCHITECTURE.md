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
- `i18n.rs` — `t!(lang, "ru", "en")` plus the Russian → Ukrainian table in
  `i18n/uk.rs`; a missing row falls back to Russian and the
  `every_string_written_in_a_call_has_a_ukrainian_row` test catches it.
  `i18n/packs.rs` loads user packs from `<config>/sd-widgets/lang`, keeps the
  active table as leaked `&'static str` pairs so `tr` never allocates, and
  produces the template from the Ukrainian keys (every translatable line).
- `ha.rs` — Home Assistant helpers.
- `update.rs` — release checks and downloads. `Remote` is the source: a
  Forgejo/Gitea instance (`/api/v1/repos/...`) or GitHub (`/api.github.com` or
  an enterprise host's `/api/v3`), read from a repository web address; both
  kinds answer the same release fields, so one parser serves them.
- `addons.rs` — declarative addon manifests, install/remove, settings values,
  `AddonInfo` for the status payload.
- `util.rs` — paths, process helpers, addresses, platform detection.

### `sdw-deck` (the Deck)

- `app.rs` — state, polling, pairing, page routing, chrome (top bar, arrows, dock,
  page transition), key input, screen sharing (`pump_share`: an egui screenshot,
  halved and JPEG-packed off the ui thread while the daemon asks for it).
  Notification cards are pulled aside on raw pointer input, before egui, so a
  sideways swipe dismisses the card while a vertical drag still scrolls. A card
  is unread until it is tapped, swiped away or covered by "read all"; the badge
  is recomputed from those cards, so the two never disagree. That state lives
  only on the Deck, the daemon keeps the actual notifications.
  The control centre lists unread notifications only: reading one (or pressing
  "read all") takes it out of the drawer for good, while the notifications page
  keeps the whole list and dims what has been read.
  The control centre is an `egui::Modal`: its dim layer lives inside the same
  egui layer as the panel, so a tap outside closes it and no area can ever end
  up between the dim layer and the panel and swallow a button press.
- `pages.rs` — home grid and every page, including the Look page.
- `clock.rs` — the four clock renderers and their controls.
- `timers.rs` — the countdown, the lap stopwatch and the pomodoro clock; their
  lengths live in `TimerCfg`, a running clock only in the page state.
- `addons_ui.rs` — the Addons page: import, enable, settings, themes, clocks and
  daemon action buttons.
- `theme.rs` — Grid tokens, CSS variable parser, presets, backdrop, tiles,
  arrows, switches, chips. A slider is a rail painted by hand with a single
  handle: only the knob senses the pointer, so a scroll that starts on the rail
  moves the page instead of the value, and a drag that starts on the knob and
  goes up or down does not change the number either.
- `deck_view.rs` — the drawn Steam Deck and live control highlighting.
- `binds.rs` — bind list, editor and capture.
- `input.rs` — evdev reader, gestures and button codes.
- `sound.rs` — synthesized WAV packs and playback.
- `viz.rs` — the sound meter: Goertzel bands from the PCM stream, a track
  fallback, and the bars/dots/wave strip shared by the top bar, the shade and
  the clock face.
- `sys.rs` — the Deck's own output volume and panel brightness (`wpctl`/`pactl`,
  sysfs/`brightnessctl`), used by the pull-down panel.
- `components.rs` — the external programs the app drives (`pw-play`/`paplay`,
  `parec`, `playerctl`, `wtype`, `xdotool`, `grim`), their declared package
  names per package manager, and an install job that runs `sudo -S` with the
  password from the settings page. On SteamOS `steamos-readonly` is turned off
  for the install and back on afterwards, even on failure. Only the ids in the
  module's table can be installed; the page never passes a command.
- `main.rs` — CLI and eframe entry.

### `sdw-daemon` (the computer)

- `server.rs` — `tiny_http` router, auth middleware, all REST handlers.
- `state.rs` — shared state, logger, job manager, Home Assistant runtime.
- `viiper.rs` — VIIPER client, pad lifecycle, password handling. The API key
  lives where the running server reads it: `/etc/viiper/viiper.key.txt` for a
  root system service, otherwise `$XDG_CONFIG_HOME/github.com/Alia5/viiper/`.
  A root service's file is unreadable to the daemon, so the password typed into
  the settings is what the client uses; a key left in the user's directory is
  ignored while a root server runs, because it belongs to another server and
  would only earn a 401.
- `monitor.rs` — monitor creation per backend, layout, config patching, capture,
  RFB client, frame buffer. niri gets a native virtual output when the running
  compositor offers `create-virtual-output` (biri), otherwise a VKMS connector.
- `mirror.rs` — the decks that send their screen, the newest frame and the
  `watching` window that decides whether they send at all. A frame goes at the
  deck's own resolution, up to 60 fps, one in flight at a time. No network, no
  blocking.
- `deck_audio.rs` — the deck's own sound on the computer: raw `s16le` chunks go
  into one `pw-cat`/`paplay` process, which is closed 1.5 s after the last one.
- `notifications.rs` — desktop notification watcher. It advertises `body-markup`,
  so the markup is reduced to plain text here (entities, `<br>`, `<img alt>`,
  D-Bus octal escapes) once, and the Deck only renders UTF-8.
- `apps.rs` — installed applications, declared power actions and the usage file
  that orders the Deck's application list by frecency.
- `bluetooth.rs` — BlueZ status, declared device actions and battery lookup.
- `audio.rs` — `parec` for the default monitor, one `pw-record` per picked
  application with a Rust mixer (PulseAudio falls back to a null sink), a
  bounded PCM ring buffer and the blocking `Read` used by the HTTP stream.
- `server.rs` also routes `addon:<id>:<action>` through the declared action
  runner; disabled addons and undeclared actions are rejected.
- `install.rs` — dependency detection and install jobs. On Windows the list is
  `ffmpeg` and the Virtual Display Driver, and the job prints the matching
  `winget` lines instead of unpacking a driver itself.
- `win/` — the Windows layer, chosen with `#[cfg(windows)]` at the crate root:
  `display.rs` (Virtual Display Driver, monitor placement), `audio.rs` (WASAPI
  loopback, per-process capture), `media.rs` (system media transport controls),
  `bluetooth.rs`, `notify.rs` (toast mirroring through
  `UserNotificationListener`) and `tray.rs` (a hidden window plus
  `Shell_NotifyIconW`). The rest of the daemon — API, model, window — is shared.
- `gui/` — hoplite-style hard-edge egui setup wizard and control panel; the
  **steam deck** page holds the mirror's `watching` window open only while its
  watch switch is on.
- `config.rs` — daemon config in TOML; `update_remote` is the release source the
  window and `--check-updates` use.

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

## Release source

Both binaries store the source as one string, `update_remote` in their own
config (empty means the built-in `git.entitybtw.ru`). A repository web address
is enough: the host decides the kind — anything with `github` in it is GitHub,
the rest is Forgejo/Gitea — and the API path and release page follow from it.
`sdw_core::update::Remote::parse` also accepts a `.git` suffix, a trailing
`/releases`, an API address and the `git@host:owner/name` form a `git remote`
prints. The check, the notes, the asset pick and the download are unchanged;
only the address moves. The daemon's `--check-updates` and `--update` read the
same field.

## Home Assistant servers on the Deck

`DeckConfig.ha_instances` is a list of `HaInstance` (`id`, `name`, `url`,
`token`); `ha_active` names the chosen one and an empty id means the computer's
connection. The old single `ha_url`/`ha_token` pair is migrated into the first
instance on load and cleared, so the two can never disagree. Switching a source
clears the cached entities, areas and views, because they belong to the server
that was asked. A chosen id whose server was removed falls back to the first
one. The token never leaves the Deck; the computer's own connection still needs
no token here.

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
- Typography: Archivo for UI, JetBrains Mono for labels and data, and a bundled
  monochrome Noto Emoji as the fallback for symbols Archivo does not carry.

## Failure modes

- Daemon unreachable → Deck keeps working locally and shows `offline`; tiles that
  need the PC are dimmed.
- VIIPER missing → gamepad tab explains what to install and links the install job.
- Monitor backend unsupported → mirror fallback, with the exact config line shown.
- No notification daemon → notifications page is empty but healthy.
