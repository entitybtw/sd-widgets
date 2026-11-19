#!/usr/bin/env python3
"""Add sd-widgets to the Steam library as a non-Steam shortcut.

Steam has no command line for this, so the script appends one entry to
`userdata/<id>/config/shortcuts.vdf` (a binary VDF file), copies the library
artwork into `userdata/<id>/config/grid/` and prints the computed app id.

    python3 steam_shortcut.py add --exe ~/.local/bin/sdw-deck \
        --name sd-widgets --start-dir ~ --icon icon.png \
        --art assets/steam [--userdata DIR] [--force]

    python3 steam_shortcut.py list

Steam rewrites `shortcuts.vdf` from memory when it exits, so it must be closed
while the file is edited. The installer handles that; the check here is the last
line of defence.
"""

from __future__ import annotations

import argparse
import os
import shutil
import struct
import sys
import time
import zlib
from pathlib import Path

TYPE_MAP = 0x00
TYPE_STRING = 0x01
TYPE_INT32 = 0x02
TYPE_UINT64 = 0x07
TYPE_END = 0x08


class VdfError(Exception):
    pass


def read_cstr(data: bytes, offset: int) -> tuple[str, int]:
    end = data.find(b"\x00", offset)
    if end < 0:
        raise VdfError("unterminated string")
    return data[offset:end].decode("utf-8", "replace"), end + 1


def parse(data: bytes) -> list[dict[str, str | int]]:
    """Parse a shortcuts.vdf file into a list of entries, in file order."""
    if not data:
        return []
    if data[0] != TYPE_MAP:
        raise VdfError("not a binary VDF file")
    key, i = read_cstr(data, 1)
    if key != "shortcuts":
        raise VdfError(f"unexpected root key {key!r}")
    entries: list[dict[str, str | int]] = []
    current: dict[str, str | int] | None = None
    while i < len(data):
        kind = data[i]
        i += 1
        if kind == TYPE_END:
            break
        if kind == TYPE_MAP:
            _, i = read_cstr(data, i)
            current = {}
            entries.append(current)
        elif kind == TYPE_STRING:
            name, i = read_cstr(data, i)
            value, i = read_cstr(data, i)
            if current is not None:
                current[name] = value
        elif kind == TYPE_INT32:
            name, i = read_cstr(data, i)
            (value,) = struct.unpack_from("<i", data, i)
            i += 4
            if current is not None:
                current[name] = value
        elif kind == TYPE_UINT64:
            name, i = read_cstr(data, i)
            (value,) = struct.unpack_from("<Q", data, i)
            i += 8
            if current is not None:
                current[name] = value
        else:
            raise VdfError(f"unknown value type 0x{kind:02x} at byte {i - 1}")
    return entries


def dump(entries: list[dict[str, str | int]]) -> bytes:
    out = bytearray(b"\x00shortcuts\x00")
    for index, entry in enumerate(entries):
        out += TYPE_MAP.to_bytes(1, "little") + str(index).encode() + b"\x00"
        for name, value in entry.items():
            key = name.encode() + b"\x00"
            if isinstance(value, str):
                out += TYPE_STRING.to_bytes(1, "little") + key + value.encode() + b"\x00"
            elif isinstance(value, bool):
                out += TYPE_INT32.to_bytes(1, "little") + key + struct.pack("<i", int(value))
            elif isinstance(value, int):
                if value >= 1 << 31:
                    out += TYPE_UINT64.to_bytes(1, "little") + key + struct.pack("<Q", value)
                else:
                    out += TYPE_INT32.to_bytes(1, "little") + key + struct.pack("<i", value)
            else:
                raise VdfError(f"cannot store {name!r} of type {type(value).__name__}")
        out += bytes([TYPE_END])
    out += bytes([TYPE_END])
    return bytes(out)


def steam_appid(entry: dict[str, str | int]) -> int:
    """The id Steam shows for a non-Steam shortcut (also names the grid art)."""
    key = f"{entry.get('Exe', '')}{entry.get('AppName', '')}"
    return (zlib.crc32(key.encode("utf-8")) & 0xFFFFFFFF) | 0x80000000


def steam_appid_variants(exe: str, name: str) -> set[int]:
    """Steam reads `Exe` with quotes; some tools compute the id without them.

    Artwork is copied under both ids so the cover appears either way.
    """
    ids = set()
    for key in (f'"{exe}"{name}', f"{exe}{name}"):
        ids.add((zlib.crc32(key.encode("utf-8")) & 0xFFFFFFFF) | 0x80000000)
    return ids


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


def userdata_dirs(explicit: str | None) -> list[Path]:
    if explicit:
        return [Path(explicit).expanduser()]
    home = Path.home()
    roots = [
        home / ".steam/steam/userdata",
        home / ".steam/root/userdata",
        home / ".local/share/Steam/userdata",
        home / ".var/app/com.valvesoftware.Steam/data/Steam/userdata",
    ]
    seen: set[Path] = set()
    found: list[Path] = []
    for root in roots:
        if not root.is_dir():
            continue
        for child in sorted(root.iterdir()):
            if not child.is_dir() or not child.name.isdigit():
                continue
            real = child.resolve()
            if real in seen:
                continue
            seen.add(real)
            found.append(child)
    return found


def current_shortcut(entries: list[dict[str, str | int]], name: str, exe: str) -> int | None:
    for i, entry in enumerate(entries):
        if entry.get("AppName") == name:
            return i
    for i, entry in enumerate(entries):
        if entry.get("Exe") in (exe, f'"{exe}"'):
            return i
    return None


def add_shortcut(
    path: Path,
    exe: str,
    name: str,
    start_dir: str,
    icon: str,
    art: Path | None,
    icon_png: Path | None,
) -> tuple[int, str]:
    old = path.read_bytes() if path.exists() else b""
    entries = parse(old)
    quoted = f'"{exe}"'
    index = current_shortcut(entries, name, exe)
    entry: dict[str, str | int] = {
        "AppName": name,
        "Exe": quoted,
        "StartDir": f'"{start_dir}"',
        "icon": icon,
        "ShortcutPath": "",
        "LaunchOptions": "",
        "IsHidden": 0,
        "AllowDesktopConfig": 1,
        "AllowOverlay": 1,
        "OpenVR": 0,
        "Devkit": 0,
        "DevkitGameID": "",
        "LastPlayTime": 0,
    }
    action = "updated"
    if index is None:
        entries.append(entry)
        index = len(entries) - 1
        action = "added"
    else:
        # Keep unknown fields Steam may have added, only point it at our files.
        merged = dict(entries[index])
        merged.update(entry)
        entries[index] = merged

    new = dump(entries)
    path.parent.mkdir(parents=True, exist_ok=True)
    if old and old != new:
        stamp = time.strftime("%Y%m%d-%H%M%S")
        backup = path.with_name(f"{path.name}.bak-{stamp}")
        backup.write_bytes(old)

    tmp = path.with_name(path.name + ".new")
    tmp.write_bytes(new)
    tmp.replace(path)

    appid = steam_appid(entries[index])
    ids = steam_appid_variants(exe, name)
    ids.add(appid)
    grid = path.parent / "grid"
    if art and art.is_dir():
        grid.mkdir(parents=True, exist_ok=True)
        for source, suffix in (
            ("grid.png", ".png"),
            ("hero.png", "_hero.png"),
            ("logo.png", "_logo.png"),
        ):
            src = art / source
            if not src.is_file():
                continue
            for value in ids:
                shutil.copyfile(src, grid / f"{value}{suffix}")
    if icon_png and icon_png.is_file():
        grid.mkdir(parents=True, exist_ok=True)
        for value in ids:
            shutil.copyfile(icon_png, grid / f"{value}_icon.png")
    return appid, action


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)

    add = sub.add_parser("add", help="add or update the sd-widgets shortcut")
    add.add_argument("--exe", required=True, help="launcher or AppImage to run")
    add.add_argument("--name", default="sd-widgets")
    add.add_argument("--start-dir", default=str(Path.home()))
    add.add_argument("--icon", default="", help="icon path stored in the entry")
    add.add_argument("--icon-png", default="", help="png copied to grid/<appid>_icon.png")
    add.add_argument("--art", default="", help="folder with grid.png, hero.png, logo.png")
    add.add_argument("--userdata", default=None, help="explicit userdata directory")
    add.add_argument("--force", action="store_true", help="write even if Steam is running")

    listing = sub.add_parser("list", help="print current shortcuts")
    listing.add_argument("--userdata", default=None, help="explicit userdata directory")

    args = parser.parse_args()

    if args.command == "list":
        for directory in userdata_dirs(args.userdata):
            file = directory / "config" / "shortcuts.vdf"
            if not file.is_file():
                continue
            print(f"# {file}")
            for entry in parse(file.read_bytes()):
                print(f"  {steam_appid(entry)}  {entry.get('AppName')}  {entry.get('Exe')}")
        return 0

    if steam_running() and not args.force:
        print(
            "Steam is running; close it first so it does not overwrite shortcuts.vdf",
            file=sys.stderr,
        )
        return 3

    exe = os.path.expanduser(args.exe)
    if not os.path.exists(exe):
        print(f"missing executable: {exe}", file=sys.stderr)
        return 2

    dirs = userdata_dirs(args.userdata)
    if not dirs:
        print("no Steam userdata found; log in to Steam once", file=sys.stderr)
        return 4

    art = Path(args.art).expanduser() if args.art else None
    icon_png = Path(args.icon_png).expanduser() if args.icon_png else None
    for directory in dirs:
        config = directory / "config"
        if not config.is_dir():
            # A userdata folder without config belongs to a profile Steam never
            # finished creating; skip it instead of inventing files.
            if not args.userdata:
                continue
        appid, action = add_shortcut(
            config / "shortcuts.vdf",
            exe,
            args.name,
            args.start_dir,
            args.icon,
            art,
            icon_png,
        )
        print(f"{action}: {args.name} ({appid}) in {config}")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except VdfError as error:
        print(f"shortcuts.vdf: {error}", file=sys.stderr)
        raise SystemExit(5)
