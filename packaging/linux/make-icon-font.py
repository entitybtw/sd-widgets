#!/usr/bin/env python3
"""Build the bundled Material Symbols font.

`assets/fonts/MaterialSymbolsOutlined.ttf` is a *subset*: it only carries the
symbols the two applications actually use, pinned to a static 400 weight. The
upstream variable font is 11 MB and would bloat both AppImages for nothing.

The codepoint list is read from the Rust sources (`\\u{...}` escapes), so adding
`pub const STAR: &str = "\\u{f09a}";` and rerunning this script is enough.

    packaging/linux/make-icon-font.py            # rewrite the subset
    packaging/linux/make-icon-font.py --check    # fail if it is stale

Requires `fonttools` (`pip install fonttools`). The upstream font is cached in
`$XDG_CACHE_HOME/sdw-widgets/tools`, so reruns are offline.
"""

from __future__ import annotations

import os
import re
import sys
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent
OUT = ROOT / "assets" / "fonts" / "MaterialSymbolsOutlined.ttf"
URL = (
    "https://raw.githubusercontent.com/google/material-design-icons/master/"
    "variablefont/MaterialSymbolsOutlined%5BFILL%2CGRAD%2Copsz%2Cwght%5D.ttf"
)

# Axes pinned for the static instance. The app always draws outlined, regular
# weight symbols, so the variable axes buy nothing at runtime.
AXES = {"FILL": 0, "GRAD": 0, "opsz": 24, "wght": 400}

# A few symbols that are not referenced yet but belong to features that exist
# (or are about to), so a new button does not need a font rebuild first.
# Names are the upstream glyph names, verified against the variable font.
EXTRA = {
    0xE8B6: "search",
    0xE5CA: "check",
    0xE5D5: "refresh",
    0xE14D: "content_copy",
    0xE30A: "computer",
    0xE1A8: "bluetooth_connected",
    0xE3C9: "edit",
    0xE835: "check_box_outline_blank",
    0xE310: "headphones",
    0xE8BD: "settings_brightness",
    0xEF44: "bedtime",
    0xE2C4: "download",
    0xE8F4: "visibility",
    0xE9F5: "toggle_off",
    0xE8B3: "history",
    0xE88E: "info",
    0xE87D: "favorite",
}


def cache_dir() -> Path:
    base = os.environ.get("XDG_CACHE_HOME") or str(Path.home() / ".cache")
    path = Path(base) / "sdw-widgets" / "tools"
    path.mkdir(parents=True, exist_ok=True)
    return path


def upstream() -> Path:
    path = cache_dir() / "MaterialSymbolsOutlined-full.ttf"
    if not path.exists():
        print(f"downloading {URL}")
        urllib.request.urlretrieve(URL, path)
    return path


def wanted() -> set[int]:
    """Every `\\u{...}` escape in the Rust workspace, plus the EXTRA list."""
    codes = set(EXTRA)
    pattern = re.compile(r"\\u\{([0-9a-fA-F]{4,6})\}")
    for path in list((ROOT / "crates").rglob("*.rs")):
        try:
            text = path.read_text(encoding="utf-8")
        except OSError:
            continue
        for match in pattern.finditer(text):
            codes.add(int(match.group(1), 16))
    return codes


def build() -> bytes:
    from fontTools import subset
    from fontTools.ttLib import TTFont
    from fontTools.varLib.instancer import instantiateVariableFont

    font = TTFont(upstream())
    font = instantiateVariableFont(font, AXES, inplace=False)

    codes = sorted(c for c in wanted() if c in font.getBestCmap())
    options = subset.Options()
    options.layout_features = []
    options.name_IDs = ["*"]
    options.name_legacy = True
    options.notdef_outline = True
    options.recalc_bounds = True
    options.drop_tables += ["BASE", "JSTF", "DSIG", "MVAR", "STAT", "fvar", "gvar"]
    subsetter = subset.Subsetter(options=options)
    subsetter.populate(unicodes=codes)
    subsetter.subset(font)

    import io

    # A reproducible file: otherwise `--check` fails on every run because the
    # `head` table carries the current timestamp.
    font.recalcTimestamp = False
    if "head" in font:
        font["head"].created = 0
        font["head"].modified = 0

    buf = io.BytesIO()
    font.save(buf)
    return buf.getvalue()


def main() -> int:
    check = "--check" in sys.argv[1:]
    data = build()
    if check:
        old = OUT.read_bytes() if OUT.exists() else b""
        if old != data:
            print("assets/fonts/MaterialSymbolsOutlined.ttf is stale; rerun", file=sys.stderr)
            return 1
        print("icon font is up to date")
        return 0
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_bytes(data)
    print(f"wrote {OUT} ({len(data) // 1024} KB, {len(wanted())} codepoints requested)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
