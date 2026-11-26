#!/usr/bin/env python3
"""Add sd-widgets to the Steam library as a non-Steam shortcut.

Steam has no command line for this, so the script appends one entry to
`userdata/<id>/config/shortcuts.vdf` (a binary VDF file), copies the library
artwork into `userdata/<id>/config/grid/` and prints the computed app id.

    python3 steam_shortcut.py add --exe ~/.local/bin/sdw-deck \
        --name sd-widgets --start-dir ~ --icon icon.png \
        --art assets/steam [--userdata DIR] [--force]

    python3 steam_shortcut.py list
    python3 steam_shortcut.py restore [--dry-run]

`add` keeps a timestamped backup of the old file (`shortcuts.vdf.bak-*`) before
it writes. `restore` puts the backup with the most shortcuts back, which undoes
a bad rewrite of the list. Steam rewrites `shortcuts.vdf` from memory when it
exits, so it must be closed while the file is edited. The installer handles
that; the check here is the last line of defence.
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
TYPE_FLOAT32 = 0x03
TYPE_POINTER = 0x04
TYPE_WIDESTRING = 0x05
TYPE_COLOR = 0x06
TYPE_UINT64 = 0x07
TYPE_END = 0x08


class VdfError(Exception):
    pass


class UInt64(int):
    """A value Steam stored as uint64 even though it also fits int32."""

    __slots__ = ()


class Raw:
    """A value of a type this tool does not interpret, kept byte for byte."""

    __slots__ = ("kind", "data")

    def __init__(self, kind: int, data: bytes) -> None:
        self.kind = kind
        self.data = data

    def __eq__(self, other: object) -> bool:
        return (
            isinstance(other, Raw)
            and other.kind == self.kind
            and other.data == self.data
        )

    def __repr__(self) -> str:
        return f"Raw(0x{self.kind:02x}, {self.data!r})"


def read_cstr(data: bytes, offset: int) -> tuple[str, int]:
    end = data.find(b"\x00", offset)
    if end < 0:
        raise VdfError("unterminated string")
    return data[offset:end].decode("utf-8", "replace"), end + 1


def read_wstr(data: bytes, offset: int) -> tuple[bytes, int]:
    start = offset
    while offset + 1 < len(data):
        if data[offset] == 0 and data[offset + 1] == 0:
            return data[start : offset + 2], offset + 2
        offset += 2
    raise VdfError("unterminated wide string")


def read_map(data: bytes, offset: int) -> tuple[dict[str, object], int]:
    """Read one map body (everything after its type/key) up to its end byte.

    Shortcuts nest: an entry holds a `tags` map, so a flat loop would stop at
    the wrong closing byte and silently drop every shortcut after the first.
    """
    items: dict[str, object] = {}
    while True:
        if offset >= len(data):
            raise VdfError("unexpected end of file")
        kind = data[offset]
        offset += 1
        if kind == TYPE_END:
            return items, offset
        name, offset = read_cstr(data, offset)
        if kind == TYPE_MAP:
            value, offset = read_map(data, offset)
        elif kind == TYPE_STRING:
            value, offset = read_cstr(data, offset)
        elif kind == TYPE_INT32:
            if offset + 4 > len(data):
                raise VdfError("truncated int32")
            (number,) = struct.unpack_from("<i", data, offset)
            value, offset = number, offset + 4
        elif kind == TYPE_UINT64:
            if offset + 8 > len(data):
                raise VdfError("truncated uint64")
            (number,) = struct.unpack_from("<Q", data, offset)
            value, offset = UInt64(number), offset + 8
        elif kind in (TYPE_FLOAT32, TYPE_POINTER, TYPE_COLOR):
            if offset + 4 > len(data):
                raise VdfError(f"truncated value of type 0x{kind:02x}")
            value, offset = Raw(kind, data[offset : offset + 4]), offset + 4
        elif kind == TYPE_WIDESTRING:
            raw, offset = read_wstr(data, offset)
            value = Raw(kind, raw)
        else:
            raise VdfError(f"unknown value type 0x{kind:02x} at byte {offset - 1}")
        if name in items:
            raise VdfError(f"duplicate key {name!r}")
        items[name] = value


def parse(data: bytes) -> list[dict[str, object]]:
    """Parse a shortcuts.vdf file into a list of entries, in file order."""
    if not data:
        return []
    if data[0] != TYPE_MAP:
        raise VdfError("not a binary VDF file")
    key, i = read_cstr(data, 1)
    if key != "shortcuts":
        raise VdfError(f"unexpected root key {key!r}")
    root, i = read_map(data, i)
    # Steam closes the root map and writes one extra end byte after it; accept
    # that terminator, but nothing else, so a truncated file cannot slip by.
    if any(byte != TYPE_END for byte in data[i:]):
        raise VdfError(f"{len(data) - i} trailing bytes after the root map")
    entries: list[dict[str, object]] = []
    for name, value in root.items():
        if not isinstance(value, dict):
            raise VdfError(f"shortcut {name!r} is not a map")
        entries.append(value)
    return entries


def write_value(out: bytearray, name: str, value: object) -> None:
    key = name.encode() + b"\x00"
    if isinstance(value, str):
        out += TYPE_STRING.to_bytes(1, "little") + key + value.encode() + b"\x00"
    elif isinstance(value, bool):
        out += TYPE_INT32.to_bytes(1, "little") + key + struct.pack("<i", int(value))
    elif isinstance(value, UInt64):
        out += TYPE_UINT64.to_bytes(1, "little") + key + struct.pack("<Q", int(value))
    elif isinstance(value, int):
        if -(1 << 31) <= value < (1 << 31):
            out += TYPE_INT32.to_bytes(1, "little") + key + struct.pack("<i", value)
        else:
            out += TYPE_UINT64.to_bytes(1, "little") + key + struct.pack("<Q", value)
    elif isinstance(value, dict):
        out += TYPE_MAP.to_bytes(1, "little") + key
        for sub_name, sub_value in value.items():
            write_value(out, sub_name, sub_value)
        out += TYPE_END.to_bytes(1, "little")
    elif isinstance(value, Raw):
        out += value.kind.to_bytes(1, "little") + key + value.data
    else:
        raise VdfError(f"cannot store {name!r} of type {type(value).__name__}")


def dump(entries: list[dict[str, object]]) -> bytes:
    out = bytearray(b"\x00shortcuts\x00")
    for index, entry in enumerate(entries):
        out += TYPE_MAP.to_bytes(1, "little") + str(index).encode() + b"\x00"
        for name, value in entry.items():
            write_value(out, name, value)
        out += TYPE_END.to_bytes(1, "little")
    out += TYPE_END.to_bytes(1, "little")
    # Steam writes a second end byte after the root map; keep the file shaped
    # exactly like one of its own.
    out += TYPE_END.to_bytes(1, "little")
    return bytes(out)


def steam_appid(entry: dict[str, object]) -> int:
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


def current_shortcut(entries: list[dict[str, object]], name: str, exe: str) -> int | None:
    for i, entry in enumerate(entries):
        if entry.get("AppName") == name:
            return i
    for i, entry in enumerate(entries):
        if entry.get("Exe") in (exe, f'"{exe}"'):
            return i
    return None


def check_rewrite(
    entries: list[dict[str, object]],
    index: int,
    entry: dict[str, object],
    data: bytes,
) -> None:
    """Refuse to touch the file unless every other shortcut survives a rewrite.

    The whole library lives in this one file, so a parse or dump mistake would
    erase every other non-Steam game. Reading our own output back proves that
    the untouched entries are byte-for-byte the same values.
    """
    again = parse(data)
    if len(again) != len(entries):
        raise VdfError(
            "refusing to write: the shortcut list would change from "
            f"{len(entries)} to {len(again)} entries"
        )
    for i, (before, after) in enumerate(zip(entries, again)):
        if i != index and before != after:
            title = before.get("AppName", f"entry {i}")
            raise VdfError(f"refusing to write: {title!r} would change")
    for name, value in entry.items():
        if again[index].get(name) != value:
            raise VdfError(f"refusing to write: field {name!r} did not survive")


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
    entry: dict[str, object] = {
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
    check_rewrite(entries, index, entry, new)
    path.parent.mkdir(parents=True, exist_ok=True)
    if old and old != new:
        stamp = time.strftime("%Y%m%d-%H%M%S")
        backup = path.with_name(f"{path.name}.bak-{stamp}")
        backup.write_bytes(old)
        if backup.read_bytes() != old:
            raise VdfError(f"could not verify the backup {backup}")

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


def restore_one(path: Path, dry_run: bool) -> bool:
    """Put back the backup that holds the most shortcuts.

    Every backup is a full copy of the file before one write, so the largest
    one is the library closest to its original state; a backup taken after a
    bad rewrite would carry only the few entries that survived it.
    """
    backups = sorted(path.parent.glob(path.name + ".bak-*"))
    if not backups:
        print(f"no backup for {path}", file=sys.stderr)
        return False
    best: tuple[int, Path] | None = None
    for candidate in backups:
        try:
            count = len(parse(candidate.read_bytes()))
        except (OSError, VdfError) as error:
            print(f"skip {candidate}: {error}", file=sys.stderr)
            continue
        if best is None or count > best[0]:
            best = (count, candidate)
    if best is None:
        return False
    count, source = best
    print(
        f"{'would restore' if dry_run else 'restored'}: {source} "
        f"({count} shortcuts) -> {path}"
    )
    if dry_run:
        return True
    if path.exists():
        stamp = time.strftime("%Y%m%d-%H%M%S")
        keep = path.with_name(f"{path.name}.before-restore-{stamp}")
        if not keep.exists():
            keep.write_bytes(path.read_bytes())
    shutil.copyfile(source, path)
    return True


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

    restore = sub.add_parser("restore", help="undo a bad shortcuts.vdf rewrite")
    restore.add_argument("--userdata", default=None, help="explicit userdata directory")
    restore.add_argument("--dry-run", action="store_true")
    restore.add_argument("--force", action="store_true", help="write even if Steam is running")

    args = parser.parse_args()

    if args.command == "restore":
        if steam_running() and not args.force:
            print(
                "Steam is running; close it first so it does not overwrite the restored list",
                file=sys.stderr,
            )
            return 3
        done = 0
        for directory in userdata_dirs(args.userdata):
            config = directory / "config"
            if not config.is_dir():
                continue
            if restore_one(config / "shortcuts.vdf", args.dry_run):
                done += 1
        if done == 0:
            print("no shortcuts.vdf backup found", file=sys.stderr)
            return 4
        return 0

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
