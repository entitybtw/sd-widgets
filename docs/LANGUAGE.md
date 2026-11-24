# Language packs

Everything the Deck app writes goes through one call in the source:

```rust
t!(lang, "Обзор", "Overview")
```

Russian and English are written next to each other, Ukrainian lives in a table
keyed by the Russian line, and a **pack** can translate the same lines into any
other language. A pack is one TOML file; no rebuild, no addon, no restart of the
daemon.

## Where packs live

```
<config>/sd-widgets/lang/*.toml
```

`<config>` is `$XDG_CONFIG_HOME` (usually `$HOME/.config`) on Linux and
`%APPDATA%` on Windows. The exact folder is printed on the **Settings** page in
the "language packs" card.

## A pack

```toml
code = "de"            # short code stored in the deck config
name = "Deutsch"       # shown on the Settings page
author = "you"         # optional

[strings]
"Обзор" = "Übersicht"
"Overview" = "Übersicht"
```

- A key may be the **Russian** or the **English** line of a `t!` call. Both
  describe the same label, so a translator picks whichever is easier.
- A line the pack leaves out keeps the built-in text (English when the pack is
  active), so a half-finished pack is still usable.
- Empty values are ignored, and a file that does not parse is listed as an
  error in the same card instead of being silently dropped.
- Two files with the same `code` are a conflict: the second one is reported and
  the first stays.

## Using one

1. Open **Settings → language packs**.
2. Press **template** — the app writes `template.toml` into the `lang` folder,
   already containing every line the app can translate.
3. Translate the right-hand sides and save the file under any name, for
   example `de.toml`. The file name is the fallback code when `code` is absent.
4. Press **reload**, then pick the pack by its name.

The choice is stored in the deck config (`lang = "de"`), so the pack is active
again on the next start. Press a built-in language chip to turn packs off, or
**turn the pack off** under the list; the built-in language comes back at once.

## Scope

Language packs belong to the Deck app. The daemon keeps its own language
setting and its own built-in tables: the two run on different machines and are
configured separately.
