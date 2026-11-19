# Themes

The Deck app is styled by CSS variables plus a small set of behaviour knobs. Both
live in `DeckConfig.style` (`ThemeCfg`) and are edited on the **Look** page.

## Variables

Declare them in a `:root { … }` block. Dark values use the plain name, light values
use the `light-` prefix.

```css
:root {
  --bg: #10141b;
  --bg2: #121820;
  --card: #18212c;
  --well: #18212c;
  --ink: #edf2f8;
  --strong: #ffffff;
  --muted: #acb9ca;
  --faint: #8493a7;
  --line: #263343;
  --line-strong: #344458;
  --accent: #a6c8ff;
  --accent-soft: #9cc3ff;

  --grid-step: 32;
  --grid-alpha: 9;

  --light-bg: #eef1f6;
  --light-bg2: #e7ebf2;
  --light-card: #ffffff;
  --light-well: #f5f7fa;
  --light-ink: #10141b;
  --light-strong: #000000;
  --light-muted: #52627a;
  --light-faint: #6c7c94;
  --light-line: #d3dae5;
  --light-line-strong: #b6c2d2;
  --light-accent: #3b6ea5;
}
```

Accepted colours: `#rgb`, `#rrggbb`, `#rrggbbaa`, `rgb(r,g,b)`,
`rgba(r,g,b,a)` (alpha `0..1` or `0..255`).

The default **grid** look is fully inspired by **Quadra**, a great hard-edge Steam
Deck theme — try it.

Rounding is **always zero** in the grid look, even if `--radius` is set: hard
edges are a product decision, not a theme option.

## Presets

| Key | Name |
| --- | --- |
| `grid` | Grid (default) |
| `mono` | Monochrome |
| `nord` | Nord |
| `gruvbox` | Gruvbox |
| `terminal` | Terminal |
| `catppuccin-mocha` | Catppuccin Mocha |
| `catppuccin-macchiato` | Catppuccin Macchiato |
| `catppuccin-frappe` | Catppuccin Frappé |
| `catppuccin-latte` | Catppuccin Latte |

Presets only replace the CSS and the grid defaults. Layout and behaviour knobs are
preserved.

Besides the built-ins, two kinds of packs appear in the same list:

- **Custom packs** — `*.css` files in `~/.config/sd-widgets/themes/`, listed as
  `custom:<file-stem>`; the Look page can save the current CSS as one.
- **Addon themes** — an installed addon with a `theme` file, listed as
  `addon:<id>`. Extra CSS from the manifest is appended. See
  [`ADDONS.md`](ADDONS.md).

New presets can also be added from the UI: edit the CSS, name it, save. No
rebuild is needed for custom or addon packs.

## Layout knobs (`ThemeCfg`)

```json
{
  "css": ":root { … }",
  "radius": 0,
  "grid": true,
  "grid_step": 32,
  "grid_alpha": 9,
  "density": 1.0,
  "font_scale": 1.0,
  "tile_height": 176,
  "tile_columns": 3,
  "arrows": true,
  "dock": true,
  "labels": true,
  "glyphs": true,
  "animations": true,
  "compact": false,
  "pages": [],
  "home_tiles": []
}
```

- `grid_step` / `grid_alpha` are mirrored into `--grid-step` / `--grid-alpha`
  when edited from the UI.
- `density` scales spacing and control padding (0.65–1.9).
- `font_scale` scales the text styles (0.75–1.8).
- `pages` is the list of page keys shown in the dock and the arrow walk; empty
  means all.
- `home_tiles` is the list of home tile keys; empty means all.

## Sounds (`SoundCfg`)

```json
{
  "enabled": true,
  "volume": 70,
  "pack": "grid",
  "click": "",
  "navigate": "",
  "open": "",
  "back": "",
  "notify": "",
  "error": "",
  "startup": ""
}
```

- Built-in packs (`grid`, `soft`, `chip`) are synthesized to small mono WAV
  files in the cache directory on first use. Volume is baked into the cached file.
- Any event can point at a custom file instead; an existing file wins over the
  pack.
- Playback uses the first available player: `pw-play`, `paplay`, `aplay`,
  `ffplay`, `mpv`.

## Adding a preset

1. Add the CSS constant in `crates/sdw-core/src/model.rs`.
2. Register it in `theme::PRESETS` and `theme::preset_css`.
3. Optionally set grid defaults in `theme::apply_preset`.
