#!/usr/bin/env python3
"""Give the sd-widgets Steam shortcut its own Steam Input layout.

Steam Input means well: for a non-Steam game it applies a "Gamepad with
Joystick Trackpad" layout, so both Deck touchpads become mouse/joystick sources.
sd-widgets reads the touchpads from the kernel itself (HAT0/HAT1), so that
translation is pure interference: pad touches move a cursor, pad clicks arrive
as mouse clicks, and bind capture sees two sources at once.

This script writes a Neptune (Steam Deck) layout named "sd-widgets" with one
active action set, "Main". Everything a normal pad has stays on xinput, but
both trackpads are deliberately left unbound: they belong to the app. The
layout is made the current one for the shortcut by an `autosave` entry in
`configset_controller_neptune.vdf`.

    python3 steam_layout.py install --name sd-widgets \
        --file packaging/linux/controller_neptune_sdw.vdf [--force] [--dry-run]

Steam rewrites its controller configs from memory, so it has to be closed while
they are edited. The installer handles that; the check here is the last line of
defence. Existing files are backed up once as `*.sdw-backup` and can be put back
by hand if the layout is not wanted.

The script follows the same Steam roots as `steam_shortcut.py`, including the
Flatpak install. It only touches the profiles whose Steam Controller Configs
folder already exists; Steam creates that folder itself on first controller
use.
"""

from __future__ import annotations

import argparse
import os
import re
import shutil
import sys
from pathlib import Path

CONFIGSET = "configset_controller_neptune.vdf"
LAYOUT = "controller_neptune.vdf"
CREATOR = "__CREATOR__"
REVISION = "__REVISION__"
STEAMID64_BASE = 76561197960287930


class LayoutError(Exception):
    pass


def steam_roots() -> list[Path]:
    home = Path.home()
    roots = [
        home / ".local/share/Steam",
        home / ".steam/steam",
        home / ".steam/root",
        home / ".var/app/com.valvesoftware.Steam/data/Steam",
        home / ".var/app/com.valvesoftware.Steam/.local/share/Steam",
    ]
    seen: set[Path] = set()
    found: list[Path] = []
    for root in roots:
        if not root.is_dir():
            continue
        real = root.resolve()
        if real in seen:
            continue
        seen.add(real)
        found.append(root)
    return found


def steam_running() -> bool:
    for proc in Path("/proc").iterdir():
        if not proc.name.isdigit():
            continue
        try:
            if (proc / "comm").read_text().strip() == "steam":
                return True
        except OSError:
            continue
    return False


def accounts(root: Path) -> list[tuple[str, Path]]:
    """(account id, controller config folder) for profiles Steam already knows."""
    userdata = root / "userdata"
    if not userdata.is_dir():
        return []
    found: list[tuple[str, Path]] = []
    for child in sorted(userdata.iterdir()):
        if not child.is_dir() or not child.name.isdigit():
            continue
        config = (
            root
            / "steamapps"
            / "common"
            / "Steam Controller Configs"
            / child.name
            / "config"
        )
        if config.is_dir():
            found.append((child.name, config))
    return found


def creator_for(account: str) -> str:
    try:
        return str(STEAMID64_BASE + int(account))
    except ValueError:
        return "0"


def normalize(text: str) -> str:
    """Compare layouts without their per-install revision and creator."""
    text = re.sub(r'("revision"\s+")[0-9]+(")', r"\1R\2", text)
    text = re.sub(r'("creator"\s+")[0-9]+(")', r"\1C\2", text)
    return text


def number(text: str, key: str) -> int | None:
    match = re.search(r'"' + key + r'"\s+"([0-9]+)"', text)
    return int(match.group(1)) if match else None


def check_layout(text: str) -> None:
    if text.count("{") != text.count("}"):
        raise LayoutError("unbalanced braces")
    if '"controller_mappings"' not in text:
        raise LayoutError("not a controller_mappings file")
    if '"controller_neptune"' not in text:
        raise LayoutError("not a Neptune (Steam Deck) layout")


def read_text(path: Path) -> tuple[str, bool]:
    raw = path.read_bytes()
    bom = raw.startswith(b"\xef\xbb\xbf")
    return raw.decode("utf-8-sig"), bom


def write_text(path: Path, text: str, bom: bool) -> None:
    data = text.encode("utf-8")
    if bom:
        data = b"\xef\xbb\xbf" + data
    temporary = path.with_name(path.name + f".tmp.{os.getpid()}")
    temporary.write_bytes(data)
    temporary.replace(path)


def entry_block(key: str, newline: str = "\n") -> str:
    lines = [f'\t"{key}"', "\t{", '\t\t"autosave"\t\t"1"', "\t}"]
    return newline.join(lines) + newline


def find_entry(lines: list[str], key: str) -> tuple[int, int] | None:
    """Line range of a flat `key { ... }` block inside the root, or None."""
    depth = 0
    wanted = f'"{key}"'
    start = None
    for i, line in enumerate(lines):
        stripped = line.strip()
        if depth == 1 and start is None and stripped == wanted:
            start = i
            continue
        if stripped == "{":
            depth += 1
        elif stripped == "}":
            depth -= 1
            if start is not None and depth == 1:
                return start, i
    return None


def ensure_autosave(path: Path, key: str, dry_run: bool) -> bool:
    text, bom = read_text(path)
    lines = text.splitlines(keepends=True)
    block = find_entry(lines, key)
    new_block = entry_block(key, "\r\n" if "\r\n" in text else "\n")
    if block is not None:
        first, last = block
        if "".join(lines[first : last + 1]) == new_block:
            return False
        if not dry_run:
            lines[first : last + 1] = [new_block]
            backup = path.with_name(path.name + ".sdw-backup")
            if not backup.exists():
                shutil.copyfile(path, backup)
            write_text(path, "".join(lines), bom)
        return True

    # Insert just before the root's closing brace. The file is a flat map of
    # app keys, so there is exactly one brace at column zero at the end.
    end = len(lines) - 1
    while end >= 0 and lines[end].strip() != "}":
        end -= 1
    if end < 0:
        raise LayoutError(f"{path}: no closing brace")
    if not dry_run:
        lines[end:end] = [new_block]
        backup = path.with_name(path.name + ".sdw-backup")
        if not backup.exists():
            shutil.copyfile(path, backup)
        write_text(path, "".join(lines), bom)
    return True


def install_one(
    account: str, config: Path, key: str, layout: str, dry_run: bool
) -> int:
    configset = config / CONFIGSET
    if not configset.is_file():
        print(
            f"skip {account}: no {CONFIGSET} yet (Steam creates it on first use)",
            file=sys.stderr,
        )
        return 0

    target = config / key / LAYOUT
    old = ""
    if target.is_file():
        old, _ = read_text(target)

    # Keep the creator of an existing file and move the revision forward: a
    # layout that goes backwards may lose to Steam's cloud copy.
    creator = creator_for(account)
    if old:
        found = number(old, "creator")
        if found is not None:
            creator = str(found)
    revision = (number(old, "revision") or 0) + 1 if old else 1

    text = layout.replace(CREATOR, creator).replace(REVISION, str(revision))
    check_layout(text)

    changed = not old or normalize(old) != normalize(text)
    if changed and not dry_run and old:
        backup = target.with_name(target.name + ".sdw-backup")
        if not backup.exists():
            shutil.copyfile(target, backup)
    if changed and not dry_run:
        target.parent.mkdir(parents=True, exist_ok=True)
        write_text(target, text, False)
    print(
        f"{'would write' if dry_run else 'wrote'}: {target}"
        + ("" if changed else " (already current)")
    )

    selected = ensure_autosave(configset, key, dry_run)
    print(
        f"{'would select' if dry_run else 'selected'}: {key} in {configset}"
        + ("" if selected else " (already)")
    )
    return 1


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)

    install = sub.add_parser("install", help="write the layout and select it")
    install.add_argument("--name", default="sd-widgets", help="Steam shortcut name")
    install.add_argument("--file", required=True, help="layout .vdf to install")
    install.add_argument("--force", action="store_true", help="write while Steam runs")
    install.add_argument("--dry-run", action="store_true")

    status = sub.add_parser("status", help="show which layout the shortcut uses")
    status.add_argument("--name", default="sd-widgets")

    args = parser.parse_args()
    key = args.name.strip().lower()
    if not key:
        print("empty shortcut name", file=sys.stderr)
        return 2

    if args.command == "status":
        header = f'"{key}"'
        roots = steam_roots()
        if not roots:
            print("no Steam install found", file=sys.stderr)
            return 4
        for root in roots:
            for account, config in accounts(root):
                configset = config / CONFIGSET
                target = config / key / LAYOUT
                state = "target present" if target.is_file() else "no layout file"
                selected = "?"
                if configset.is_file():
                    text, _ = read_text(configset)
                    selected = "selected" if header in text else "not selected"
                print(f"{account}: {state}, {selected} ({target})")
        return 0

    if steam_running() and not args.force:
        print(
            "Steam is running; close it first so it does not overwrite the layout",
            file=sys.stderr,
        )
        return 3

    source = Path(args.file).expanduser()
    if not source.is_file():
        print(f"missing layout: {source}", file=sys.stderr)
        return 2
    layout, _ = read_text(source)
    check_layout(layout)

    roots = steam_roots()
    if not roots:
        print(
            "no Steam install found; the app still works, but the layout was not written",
            file=sys.stderr,
        )
        return 4

    done = 0
    for root in roots:
        for account, config in accounts(root):
            done += install_one(account, config, key, layout, args.dry_run)
    if done == 0:
        print(
            "no Steam Controller Configs folder yet; launch Steam once, then rerun with --steam-only",
            file=sys.stderr,
        )
        return 4
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except LayoutError as error:
        print(f"layout: {error}", file=sys.stderr)
        raise SystemExit(5)