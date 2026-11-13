"""Build the Mac iconset from the selected Island For Mac artwork.

Run: python3 tools/make_mac_icon.py <out.iconset> (needs Pillow), then:
     iconutil -c icns <out.iconset> -o IslandForMac.icns
"""
import sys
from pathlib import Path

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parent.parent
TILE = 824 / 1024
ICONSET = [
    ("icon_16x16.png", 16),
    ("icon_16x16@2x.png", 32),
    ("icon_32x32.png", 32),
    ("icon_32x32@2x.png", 64),
    ("icon_128x128.png", 128),
    ("icon_128x128@2x.png", 256),
    ("icon_256x256.png", 256),
    ("icon_256x256@2x.png", 512),
    ("icon_512x512.png", 512),
    ("icon_512x512@2x.png", 1024),
]


def artwork() -> Image.Image:
    source = Image.open(ROOT / "docs/brand/island-for-mac-source.png").convert("RGB")
    mask = Image.new("L", source.size)
    draw = ImageDraw.Draw(mask)
    # 仅用每行两侧的饱和色定位外轮廓，保留画面内部的浅色太阳和倒影。
    for y in range(source.height):
        edges = []
        for x in range(source.width):
            pixel = source.getpixel((x, y))
            if max(pixel) - min(pixel) > 90:
                edges.append(x)
        if edges:
            draw.line((edges[0], y, edges[-1], y), fill=255)
    source.putalpha(mask)
    return source.crop(mask.getbbox())


def main() -> int:
    if len(sys.argv) != 2:
        print(__doc__)
        return 2
    out = Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)
    image = artwork()
    for name, px in ICONSET:
        tile_px = round(px * TILE)
        tile = image.resize((tile_px, tile_px), Image.Resampling.LANCZOS)
        icon = Image.new("RGBA", (px, px))
        icon.alpha_composite(tile, ((px - tile_px) // 2, (px - tile_px) // 2))
        icon.save(out / name)

    # 设置页最大显示40点，80像素足够Retina；预乘BGRA直接交给现有绘图器。
    small = image.resize((80, 80), Image.Resampling.LANCZOS)
    pixels = bytearray()
    for r, g, b, a in small.getdata():
        pixels.extend(((b * a + 127) // 255, (g * a + 127) // 255, (r * a + 127) // 255, a))
    (ROOT / "crates/poni/assets/island-app-icon.bgra").write_bytes(pixels)
    return 0


if __name__ == "__main__":
    sys.exit(main())
