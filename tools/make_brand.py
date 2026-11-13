"""Draw Oyenza Studio's app icon, installer pictures and documentation logo.

The app icon pairs a floating blue-cyan island with a stack of files on a graphite tile. Its simple silhouette
stays readable at menu and Dock sizes. The original Oyen artwork remains unchanged in the installer's picture.

Run: python3 tools/make_brand.py  (needs Pillow)

Writes:
  crates/poni/res/poni.ico
  installer/art/wizard-small-*.png
  installer/art/wizard-*.png
  installer/art/nsis-*.bmp
  docs/brand/oyen-poni-logo.svg
  docs/brand/oyen-poni-logo-512.png
"""
import sys
from pathlib import Path

from PIL import Image, ImageChops, ImageDraw, ImageFilter

ROOT = Path(__file__).resolve().parent.parent
BRAND = ROOT / "docs" / "brand"
RES = ROOT / "crates" / "poni" / "res"
ART = ROOT / "installer" / "art"

ICO_SIZES = [16, 20, 24, 32, 40, 48, 64, 96, 128, 256]
WIZARD_SIZES = [(202, 386), (336, 643), (430, 824)]
WIZARD_SMALL_SIZES = [58, 77, 97, 116, 124, 143, 159]
INSET = 0.047
TILE_R = 0.2245


def gradient(w: int, h: int, stops: list[tuple[float, str]]) -> Image.Image:
    rgb = [(u, Image.new("RGB", (1, 1), c).getpixel((0, 0))) for u, c in stops]
    col = Image.new("RGB", (1, max(1, h)))
    for y in range(h):
        u = y / max(1, h - 1)
        (u0, a), (u1, b) = next((rgb[k], rgb[k + 1]) for k in range(len(rgb) - 1) if u <= rgb[k + 1][0] or k == len(rgb) - 2)
        t = 0 if u1 == u0 else min(1, max(0, (u - u0) / (u1 - u0)))
        col.putpixel((0, y), tuple(round(a[i] + (b[i] - a[i]) * t) for i in range(3)))
    return col.resize((max(1, w), max(1, h)))


def rounded_gradient(img: Image.Image, box: tuple[float, float, float, float], radius: float, stops: list[tuple[float, str]]) -> None:
    """Composite a rounded gradient without colouring its transparent corners."""
    x0, y0, x1, y1 = box
    w, h = round(x1 - x0), round(y1 - y0)
    layer = gradient(w, h, stops).convert("RGBA")
    mask = Image.new("L", (w, h), 0)
    ImageDraw.Draw(mask).rounded_rectangle((0, 0, w - 1, h - 1), radius=radius, fill=255)
    layer.putalpha(mask)
    img.alpha_composite(layer, (round(x0), round(y0)))


def draw_logo(size: int, ss: int = 8) -> Image.Image:
    """Draw each size independently; the smallest icons omit the file's fine lines."""
    s = size * ss
    inset = s * INSET
    t = s - 2 * inset
    tile_box = (inset, inset, inset + t, inset + t)
    tile_mask = Image.new("L", (s, s), 0)
    ImageDraw.Draw(tile_mask).rounded_rectangle(tile_box, radius=TILE_R * t, fill=255)
    img = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    rounded_gradient(img, tile_box, TILE_R * t, [(0.0, "#243548"), (1.0, "#0B111C")])

    # A quiet light behind the mark gives depth without making small icons busy.
    light = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    ImageDraw.Draw(light).ellipse((inset + t * 0.08, inset + t * 0.02, inset + t * 0.82, inset + t * 0.55), fill=(57, 177, 225, 38))
    light = light.filter(ImageFilter.GaussianBlur(t * 0.13))
    light.putalpha(ImageChops.multiply(light.getchannel("A"), tile_mask))
    img.alpha_composite(light)
    draw = ImageDraw.Draw(img)
    if size >= 32:
        draw.rounded_rectangle(tile_box, radius=TILE_R * t, outline=(193, 225, 250, 50), width=max(1, round(t * 0.003)))

    # The island floats above two staggered sheets, like the local file shelf.
    pill = (inset + t * 0.25, inset + t * 0.19, inset + t * 0.75, inset + t * 0.34)
    shadow = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    ImageDraw.Draw(shadow).rounded_rectangle((pill[0], pill[1] + t * 0.025, pill[2], pill[3] + t * 0.025), radius=t * 0.075, fill=(0, 0, 0, 115))
    img.alpha_composite(shadow.filter(ImageFilter.GaussianBlur(t * 0.025)))
    rounded_gradient(img, pill, t * 0.075, [(0.0, "#A6FFE9"), (0.45, "#55CFED"), (1.0, "#3E70F3")])
    if size >= 32:
        ImageDraw.Draw(img).rounded_rectangle(pill, radius=t * 0.075, outline=(216, 255, 255, 115), width=max(1, round(t * 0.004)))

    back = (inset + t * 0.34, inset + t * 0.44, inset + t * 0.76, inset + t * 0.74)
    rounded_gradient(img, back, t * 0.065, [(0.0, "#8ED9E9"), (1.0, "#3D7CA5")])
    front = (inset + t * 0.24, inset + t * 0.50, inset + t * 0.68, inset + t * 0.81)
    card_shadow = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    ImageDraw.Draw(card_shadow).rounded_rectangle((front[0], front[1] + t * 0.025, front[2], front[3] + t * 0.025), radius=t * 0.065, fill=(0, 0, 0, 95))
    img.alpha_composite(card_shadow.filter(ImageFilter.GaussianBlur(t * 0.018)))
    rounded_gradient(img, front, t * 0.065, [(0.0, "#FFFFFF"), (1.0, "#B7D7EB")])
    if size >= 32:
        draw = ImageDraw.Draw(img)
        draw.rounded_rectangle((inset + t * 0.32, inset + t * 0.60, inset + t * 0.59, inset + t * 0.625), radius=t * 0.0125, fill="#3B7897")
        draw.rounded_rectangle((inset + t * 0.32, inset + t * 0.69, inset + t * 0.51, inset + t * 0.715), radius=t * 0.0125, fill="#70A2BA")
    return img.resize((size, size), Image.LANCZOS)


def logo_svg() -> str:
    return '''<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1000 1000"><title>Oyenza Studio</title>
<defs>
  <linearGradient id="tile" x2="0%" y2="100%"><stop stop-color="#243548"/><stop offset="1" stop-color="#0B111C"/></linearGradient>
  <linearGradient id="island" x2="0%" y2="100%"><stop stop-color="#A6FFE9"/><stop offset=".45" stop-color="#55CFED"/><stop offset="1" stop-color="#3E70F3"/></linearGradient>
  <linearGradient id="back" x2="0%" y2="100%"><stop stop-color="#8ED9E9"/><stop offset="1" stop-color="#3D7CA5"/></linearGradient>
  <linearGradient id="sheet" x2="0%" y2="100%"><stop stop-color="#FFFFFF"/><stop offset="1" stop-color="#B7D7EB"/></linearGradient>
  <radialGradient id="light"><stop stop-color="#39B1E1" stop-opacity=".18"/><stop offset="1" stop-color="#39B1E1" stop-opacity="0"/></radialGradient>
  <filter id="shadow" x="-30%" y="-30%" width="160%" height="180%"><feDropShadow dy="20" stdDeviation="18" flood-opacity=".35"/></filter>
  <clipPath id="tile-clip"><rect x="47" y="47" width="906" height="906" rx="203.4"/></clipPath>
</defs>
<rect x="47" y="47" width="906" height="906" rx="203.4" fill="url(#tile)" stroke="#C1E1FA" stroke-opacity=".20" stroke-width="3"/>
<ellipse cx="455" cy="290" rx="350" ry="240" fill="url(#light)" clip-path="url(#tile-clip)"/>
<rect x="273.5" y="219.1" width="453" height="135.9" rx="67.95" fill="url(#island)" stroke="#D8FFFF" stroke-opacity=".45" stroke-width="4" filter="url(#shadow)"/>
<rect x="355.0" y="445.6" width="380.5" height="271.8" rx="58.9" fill="url(#back)"/>
<rect x="264.4" y="500" width="398.6" height="280.9" rx="58.9" fill="url(#sheet)" filter="url(#shadow)"/>
<rect x="336.9" y="590.6" width="244.6" height="22.7" rx="11.3" fill="#3B7897"/>
<rect x="336.9" y="672.1" width="172.1" height="22.7" rx="11.3" fill="#70A2BA"/>
</svg>\n'''


def trimmed(path: Path) -> Image.Image:
    im = Image.open(path).convert("RGBA")
    return im.crop(im.getchannel("A").point(lambda v: 255 if v > 32 else 0).getbbox())


def wizard_image(w: int, h: int, oyen: Image.Image) -> Image.Image:
    """Welcome/finish picture: Oyen on the island, playing music, on a warm cream field."""
    img = gradient(w, h, [(0.0, "#FFF6EC"), (1.0, "#FFE3C4")]).convert("RGBA")
    ow = round(w * 0.88)
    oh = round(oyen.height * ow / oyen.width)
    art = oyen.resize((ow, oh), Image.LANCZOS)
    img.alpha_composite(art, ((w - ow) // 2, round(h * 0.47 - oh / 2)))
    return img


def main() -> int:
    # App icon: every size drawn on its own.
    icons = {n: draw_logo(n) for n in ICO_SIZES}
    icons[256].save(RES / "poni.ico", format="ICO", sizes=[(n, n) for n in ICO_SIZES], append_images=[icons[n] for n in ICO_SIZES[:-1]])
    stale = RES / "poni-512.png"
    if stale.exists():
        stale.unlink()

    BRAND.joinpath("oyen-poni-logo.svg").write_text(logo_svg())
    draw_logo(512).save(BRAND / "oyen-poni-logo-512.png", optimize=True)

    ART.mkdir(exist_ok=True)
    for n in WIZARD_SMALL_SIZES:
        draw_logo(n).save(ART / f"wizard-small-{n}.png", optimize=True)
    oyen = trimmed(BRAND / "art" / "oyen-poni-sit.webp")
    for w, h in WIZARD_SIZES:
        wizard_image(w, h, oyen).convert("RGB").save(ART / f"wizard-{w}x{h}.png", optimize=True)

    # NSIS (MUI2) only takes 24-bit BMPs: 164x314 welcome/finish picture, 150x57 header.
    wizard_image(164, 314, oyen).convert("RGB").save(ART / "nsis-welcome.bmp")
    header = Image.new("RGBA", (150, 57), "white")
    header.alpha_composite(draw_logo(48), (150 - 48 - 6, (57 - 48) // 2))
    header.convert("RGB").save(ART / "nsis-header.bmp")

    for p in sorted([RES / "poni.ico", BRAND / "oyen-poni-logo.svg", BRAND / "oyen-poni-logo-512.png", *ART.iterdir()]):
        print(f"{p.relative_to(ROOT)}  {p.stat().st_size} bytes")
    return 0


if __name__ == "__main__":
    sys.exit(main())
