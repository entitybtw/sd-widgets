# Addons

An addon is a small, declarative package that extends the Deck, the daemon, or
both. There is no native code and no dynamic loading: an addon is a JSON
manifest plus optional assets (CSS, clock JSON, images, sounds). Both binaries
read the same format, so one package can ship a theme and a daemon action
together.

## Where addons live

```
~/.config/sd-widgets/addons/<id>/
├── addon.json
├── theme.css
├── clock.json
└── sounds/
    └── click.wav
```

`<id>` is the manifest `id`, lowercased. Allowed characters are letters, digits,
`-`, `_` and `.`. Everything else becomes `-`.

Each side stores only its own state:

- Deck: `enabled` and per-addon `values` in `~/.config/sd-widgets/deck.json`
  under `addons`.
- Daemon: the same fields in `~/.config/sd-widgets/daemon.toml` under
  `[addons]`.

An addon can be installed on both machines independently; they do not sync.

## Installing

From **Addons → import** on the Deck, or from the **Addons** page of the daemon
window. The path may point at:

| Source | What happens |
| --- | --- |
| a folder | Must contain `addon.json`; the whole folder is copied. |
| `addon.json` | The manifest is imported with the assets it references, relative to its folder. |
| `*.css` | A one-file theme pack: the CSS becomes `theme.css` and a minimal manifest is written. |
| `*.sdwaddon`, `*.zip` | Unpacked by both binaries themselves, then installed as a folder. |
| `http(s)` link | Downloaded (max 32 MB) and installed by content: zip, manifest or CSS. |

A `.zip`/`.sdwaddon` may keep its files at the root or inside one top-level
folder. The unpacker rejects absolute paths and `..` escapes, and refuses an
archive that expands beyond 64 MB. No external `unzip` is needed, so the same
link works on Linux and Windows.

Importing an existing `id` replaces the previous package; the user's enabled
state and values are kept. **Remove** deletes the folder and clears its values.

## Store

A store is a static JSON catalogue plus a base URL. Add the base once in
**Addons → store**; its packages appear as tiles with an **install** button.
The deck installs the package locally and asks the paired computer to install
the same URL, so a package with daemon actions works on both sides after one
press.

The app tries these files under the base, in order:

1. `repo.json`
2. `index.json`
3. `api/v1/addons.json`
4. `addons.json`

A base that already ends in `.json` is used as-is. The document may be a bare
array or an object:

```json
{
  "schema": 1,
  "name": "My addons",
  "addons": [
    {
      "id": "lamp",
      "name": "Lamp",
      "tagline": "One tile for the desk lamp",
      "description": "…",
      "version": "1.2.0",
      "author": "you",
      "scope": "both",
      "homepage": "https://example.org/lamp",
      "url": "packs/lamp.sdwaddon",
      "sha256": "…"
    }
  ]
}
```

`url` may be absolute or relative to the store base. When it is omitted, the
app assumes `extensions/<id>.sdwaddon`. When `sha256` is present the download is
checked before anything is written to the addon folder; a mismatch aborts the
install.

A store is just a static host (a Git forge release page, a web server, a
GitHub/Gitea Pages site). The app never runs code from the catalogue; it only
downloads the package the user asked for and then follows the same manifest
rules as a local file.

## Manifest

```json
{
  "id": "night-shift",
  "name": "Night shift",
  "version": "1.0.0",
  "author": "you",
  "description": "A dark theme, a clock and a test notification.",
  "homepage": "https://example.org/night-shift",
  "scope": "both",
  "deck": {
    "theme": "theme.css",
    "clock": "clock.json",
    "extra_css": "--radius: 0;",
    "tiles": [],
    "sounds": {}
  },
  "daemon": {
    "actions": []
  },
  "settings": []
}
```

All fields are optional except that the package must produce something useful.
`id` defaults to the folder name. `scope` is informational: `deck`, `daemon` or
`both`.

### `deck`

| Field | Type | Meaning |
| --- | --- | --- |
| `theme` | path | CSS file, relative to the addon folder. |
| `clock` | path | JSON serialisation of a `ClockCfg`. |
| `extra_css` | string | Extra declarations appended after the theme file. |
| `tiles` | array | Tiles added to the Deck home grid. |
| `sounds` | map | Cue name → WAV/OGG path, relative to the addon folder. |

Theme files use the same variables as [`THEMES.md`](THEMES.md). An installed
theme appears in the Look presets as `addon:<id>` and applies exactly like a
built-in preset. A clock preset appears as a button in the Clock style selector
and replaces the whole clock configuration when pressed.

Sound cue names are `click`, `navigate`, `open`, `back`, `notify`, `error` and
`startup`. When an addon is enabled, its files replace the corresponding
per-cue paths in the deck configuration.

#### Tiles

```json
{
  "title": "Open the site",
  "subtitle": "a deck-side tile",
  "icon": "\ue8b6",
  "accent": "#3fd2ff",
  "action": { "url": { "url": "https://example.org" } }
}
```

`icon` is a Material Symbols codepoint or a short text glyph; an empty value
falls back to the addon icon. `accent` is `#rrggbb`. `action` is any `Action`
(see below) and runs on the Deck, so local actions are allowed too.

### `daemon.actions`

```json
{
  "id": "notify",
  "label": "Send a test notification",
  "description": "runs on the computer",
  "confirm": false,
  "action": { "shell": { "command": "notify-send sd-widgets hello" } }
}
```

The Deck shows one button per action; pressing it sends the remote command
`addon:<id>:<action-id>` to the paired computer. The daemon window shows the same
actions on its Addons page and runs them locally. `confirm` is reserved for a
future confirmation dialog and is currently informational.

### `settings`

```json
{
  "key": "name",
  "label": "Your name",
  "kind": "text",
  "default": "deck",
  "options": [],
  "min": null,
  "max": null,
  "hint": "shown under the field"
}
```

| `kind` | Control |
| --- | --- |
| `text` | single-line text field |
| `number` | drag value, optionally bounded by `min`/`max` |
| `bool` | switch |
| `color` | colour picker, stored as `#rrggbb` |
| `choice` | one selectable label per entry in `options` |

Both applications render the same fields. Values are read with
`values[<id>][<key>]`; a missing value falls back to `default`.

### Templates

Every string in an action is expanded right before it runs. `{key}` and
`{settings.key}` both name a setting, so one package can point at any API,
entity or folder without editing its JSON:

```json
"action": {
  "http": {
    "method": "POST",
    "url": "{base}/hooks/{token}",
    "body": "{\"who\":\"{name}\",\"level\":{level}}"
  }
}
```

Unknown names are left as written, which makes a typo visible in the error
instead of silently sending an empty value. Changing a setting applies to the
next call; nothing needs reinstalling.

## Actions

Actions are the shared `Action` enum from `sdw-core`:

| Variant | JSON |
| --- | --- |
| none | `"none"` |
| shell | `{ "shell": { "command": "…" } }` |
| launch | `{ "launch": { "target": "desktop-id" } }` |
| volume | `{ "volume": { "delta": 5 } }` |
| volume set | `{ "volume_set": { "percent": 40 } }` |
| mute | `"mute"` |
| brightness | `{ "brightness": { "delta": -5 } }` |
| brightness set | `{ "brightness_set": { "percent": 60 } }` |
| media | `{ "media": { "command": "play_pause" } }` |
| key | `{ "key": { "combo": "Super_L" } }` |
| page | `{ "page": { "page": "clock" } }` |
| remote | `{ "remote": { "command": "volume_up" } }` |
| HA | `{ "ha": { "domain": "light", "service": "toggle", "entity": "light.desk" } }` |
| URL | `{ "url": { "url": "https://…" } }` |
| HTTP | `{ "http": { "method": "POST", "url": "https://…", "headers": {"Authorization": "Bearer …"}, "body": "…" } }` |

What the **daemon** accepts for an addon action:

| Action | Behaviour |
| --- | --- |
| `shell` | `sh -lc`, first stdout or stderr line becomes the result. |
| `launch` | `gtk-launch`, then `xdg-open`. |
| `url` | `http`/`https` only. |
| `volume`, `volume_set`, `mute` | `wpctl`, then `pactl`, then `amixer`. |
| `brightness`, `brightness_set` | `brightnessctl`. |
| `media` | `playerctl`. |
| `key` | `wtype -k`, then `ydotool key`. |
| `page` | refused (a Deck concept). |
| `remote` | refused (no nesting). |
| `ha` | refused (Home Assistant runs on the Deck). |
| `http` | Any `http`/`https` URL, 20 s timeout, body trimmed to a log line. Headers are sent as written; `Content-Type` defaults to `application/json` when a body is present. |

On the Deck, every action is executed by the normal bind pipeline, so a tile can
switch pages, open a URL, change volume, call Home Assistant or talk to the
computer. An `http` action runs in a worker, so a slow service never freezes the
page; its answer becomes a short toast.

## HTTP

`POST /api/v1/remote` with `{"command": "addon:night-shift:notify"}` runs a
daemon action. The addon must be installed **and enabled** in the daemon
configuration; otherwise the call returns `400`. Only actions declared in the
manifest can be reached — there is no way to pass a command.

`POST /api/v1/addons/install` with `{"url": "https://…/pack.sdwaddon"}`
downloads and installs a package on the computer and enables it. The deck's
store uses this to put a package with daemon actions on both machines.

`GET /api/v1/status` includes `addons`:

```json
{
  "id": "night-shift",
  "name": "Night shift",
  "version": "1.0.0",
  "description": "…",
  "scope": "both",
  "enabled": true,
  "actions": [{ "id": "notify", "label": "…", "description": "…", "confirm": false }],
  "tiles": 1,
  "has_theme": true,
  "has_clock": true
}
```

## Creating one from the UI

- **Deck → Addons → example** writes a complete example to
  `~/.config/sd-widgets/addons/example/addon.json`.
- **Deck → Addons → save theme** stores the current theme CSS as a theme addon.
- **Deck → Addons → save clock** stores the current `ClockCfg` as a clock addon.
- **Daemon → Addons → example** writes the same example next to the daemon
  configuration.
- **Addons → store** adds a repository base and installs any package it lists
  with one press.

Saved presets are ordinary packages: edit the files, re-import, or ship the
folder as a `.sdwaddon` (any zip with `addon.json` at the root).

## Security

- A package is inert until the user installs it and enables it.
- Disabled addons are not listed as runnable and are rejected by the daemon.
- Remote invocation can only name an existing declared action.
- `shell` is allowed for daemon addons because the user installed the package
  locally; it is still not reachable from arbitrary deck input, only from the
  declared action id.
- Nested `remote` actions and Deck-only actions are refused on the daemon, so an
  addon cannot bootstrap a second command channel.
- An `http` action can reach any `http(s)` address, including services on the
  local network. It cannot read `file://` URLs, but it is still a network call
  made on your behalf: read the manifest before enabling a package from someone
  else.
- Store catalogues are community content. Adding a base does not trust it; the
  package you press **install** on is the one that runs, and a declared
  `sha256` is verified first.

Treat an addon like any other program you install: read `addon.json` before
enabling one from someone else.
