#!/usr/bin/env python3
"""Draw the Steam library artwork for sd-widgets.

The installer copies the results into Steam's `grid` folder, so a non-Steam
shortcut looks like a real library entry. Run from the repository root:

    packaging/linux/make-steam-art.py

Requires Pillow. Writes `assets/steam/{grid,hero,logo}.png` in the Grid visual
language: hard edges, hairline lines, a 32 px rhythm and the accent blue.
"""

from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parent.parent.parent
OUT = ROOT / "assets" / "steam"
ICON = ROOT / "assets" / "icons" / "sd-widgets-512.png"
ARCHIVO = ROOT / "assets" / "fonts" / "Archivo.ttf"
MONO = ROOT / "assets" / "fonts" / "JetBrainsMono-Regular.ttf"
MONO_SEMI = ROOT / "assets" / "fonts" / "JetBrainsMono-SemiBold.ttf"

BG = (16, 20, 27, 255)
LINE = (30, 41, 55, 255)
GRID = (22, 30, 40, 255)
INK = (237, 242, 248, 255)
MUTED = (132, 147, 167, 255)
ACCENT = (166, 200, 255, 255)


def font(path: Path, size: int) -> ImageFont.FreeTypeFont:
    return ImageFont.truetype(str(path), size)


def grid(size: tuple[int, int], step: int, alpha: int = 255) -> Image.Image:
    img = Image.new("RGBA", size, BG)
    d = ImageDraw.Draw(img)
    color = (*GRID[:3], alpha)
    for x in range(0, size[0], step):
        d.line([(x, 0), (x, size[1])], fill=color)
    for y in range(0, size[1], step):
        d.line([(0, y), (size[0], y)], fill=color)
    return img


def centered(d: ImageDraw.ImageDraw, text: str, f, y: int, width: int, fill) -> None:
    left, top, right, bottom = d.textbbox((0, 0), text, font=f)
    d.text(((width - (right - left)) / 2 - left, y - top), text, font=f, fill=fill)


def spaced(
    d: ImageDraw.ImageDraw,
    text: str,
    f,
    y: int,
    width: int | None,
    fill,
    gap: int,
    left: int = 0,
) -> None:
    widths = [d.textlength(ch, font=f) for ch in text]
    total = sum(widths) + gap * (len(text) - 1)
    x = left if width is None else (width - total) / 2
    for ch, w in zip(text, widths):
        d.text((x, y), ch, font=f, fill=fill)
        x += w + gap


def grid_art() -> Image.Image:
    w, h = 600, 900
    img = grid((w, h), 40)
    d = ImageDraw.Draw(img)

    # Hairline frame and an accent strip, the way the app draws a card.
    d.rectangle([23, 23, w - 24, h - 24], outline=LINE, width=1)
    d.rectangle([0, 0, w, 6], fill=ACCENT)

    icon = Image.open(ICON).convert("RGBA").resize((224, 224), Image.LANCZOS)
    img.alpha_composite(icon, ((w - 224) // 2, 150))

    title = font(ARCHIVO, 64)
    centered(d, "sd-widgets", title, 448, w, INK)
    spaced(d, "STEAM DECK CONTROL SURFACE", font(MONO, 15), 536, w, ACCENT, 4)
    d.line([(120, 604), (w - 121, 604)], fill=LINE)

    small = font(MONO, 14)
    for i, label in enumerate(("GAMEPAD", "SCREEN", "AUDIO", "HOME")):
        col = i % 2
        row = i // 2
        x = 118 + col * 182
        y = 660 + row * 54
        d.rectangle([x, y, x + 12, y + 12], outline=ACCENT if i == 0 else MUTED, width=1)
        d.text((x + 24, y - 1), label, font=small, fill=MUTED)

    centered(d, "v" + version(), font(MONO, 13), 820, w, MUTED)
    return img.convert("RGB")


def hero_art() -> Image.Image:
    w, h = 1920, 620
    img = grid((w, h), 62, alpha=160)
    d = ImageDraw.Draw(img)
    d.rectangle([0, h - 6, w, h], fill=ACCENT)

    icon = Image.open(ICON).convert("RGBA").resize((260, 260), Image.LANCZOS)
    img.alpha_composite(icon, (150, (h - 260) // 2 - 10))

    d.text((480, 205), "sd-widgets", font=font(ARCHIVO, 108), fill=INK)
    spaced(d, "STEAM DECK CONTROL SURFACE", font(MONO, 22), 348, None, ACCENT, 6, left=482)
    d.text(
        (482, 400),
        "gamepad · extra screen · pc audio · home assistant",
        font=font(MONO, 21),
        fill=MUTED,
    )
    d.line([(482, 372), (482 + 820, 372)], fill=LINE)
    return img.convert("RGB")


def logo_art() -> Image.Image:
    w, h = 640, 360
    img = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    d.rectangle([0, 0, 14, 14], fill=ACCENT)
    d.text((36, 44), "sd-widgets", font=font(ARCHIVO, 96), fill=INK)
    spaced(d, "CONTROL SURFACE", font(MONO, 20), 176, None, MUTED, 5, left=40)
    d.line([(38, 226), (w - 40, 226)], fill=ACCENT)
    return img


def version() -> str:
    for line in (ROOT / "Cargo.toml").read_text().splitlines():
        if line.startswith("version = "):
            return line.split('"')[1]
    return "0"


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    grid_art().save(OUT / "grid.png", optimize=True)
    hero_art().save(OUT / "hero.png", optimize=True)
    logo_art().save(OUT / "logo.png", optimize=True)
    print(f"wrote {OUT}")


if __name__ == "__main__":
    main()
