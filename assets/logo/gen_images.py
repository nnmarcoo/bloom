#!/usr/bin/env python3
"""Generate bloom.ico, PNGs, and banner from bloom.svg.

bloom.svg contains only the raw "B" glyph paths (white fills, no background).
This script adds the blue rounded-rect background for icon outputs,
and composites the raw glyph with bold text for the banner.

Requirements: pip install pillow aggdraw svgelements
"""

from io import StringIO
from pathlib import Path
from PIL import Image, ImageDraw, ImageFont
import aggdraw
import svgelements as se

DIR = Path(__file__).parent
SVG_PATH = DIR / "bloom.svg"
ICO_PATH = DIR / "bloom.ico"
ICO_SIZES = [256, 128, 64, 48, 32, 16]

BLUE = (0, 132, 190)
CORNER_RADIUS_RATIO = 0.1014  # matches original SVG: 202.899 / 2000
ICON_GLYPH_RATIO = 0.78       # glyph occupies 78% of icon, matching original SVG margin

BANNER_LOGO_H = 72
BANNER_FONT_SIZE = 72
BANNER_INNER_PAD = 16
BANNER_FONT_PATHS = [
    "C:/Windows/Fonts/segoeuib.ttf",                              # Windows — Segoe UI Bold
    "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",       # Linux
    "/System/Library/Fonts/Helvetica.ttc",                        # macOS
]

_COLOR_NONE = se.Color("none")


def color_to_rgba(c: se.Color) -> tuple | None:
    if c is None or c == _COLOR_NONE:
        return None
    return (c.red, c.green, c.blue, int(c.opacity * 255))


def shape_to_aggpath(shape: se.Shape, scale: float) -> aggdraw.Path:
    p = aggdraw.Path()
    for seg in shape.segments(transformed=True):
        if isinstance(seg, se.Move):
            p.moveto(seg.end.x * scale, seg.end.y * scale)
        elif isinstance(seg, se.Line):
            p.lineto(seg.end.x * scale, seg.end.y * scale)
        elif isinstance(seg, se.CubicBezier):
            p.curveto(
                seg.control1.x * scale, seg.control1.y * scale,
                seg.control2.x * scale, seg.control2.y * scale,
                seg.end.x * scale, seg.end.y * scale,
            )
        elif isinstance(seg, se.QuadraticBezier):
            # Promote to cubic bezier
            cx, cy = seg.control.x * scale, seg.control.y * scale
            ex, ey = seg.end.x * scale, seg.end.y * scale
            sx, sy = seg.start.x * scale, seg.start.y * scale
            p.curveto(
                sx + 2/3 * (cx - sx), sy + 2/3 * (cy - sy),
                ex + 2/3 * (cx - ex), ey + 2/3 * (cy - ey),
                ex, ey,
            )
        elif isinstance(seg, se.Arc):
            # Approximate arc as chord; bloom.svg contains no arcs
            p.lineto(seg.end.x * scale, seg.end.y * scale)
        elif isinstance(seg, se.Close):
            p.close()
    return p


def render_svg(svg_content: str, size: int) -> Image.Image:
    """Render the raw SVG glyph at the given size on a transparent background."""
    svg = se.SVG.parse(StringIO(svg_content), width=size, height=size)
    vw = float(svg.width) if svg.width else size
    scale = size / vw

    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    canvas = aggdraw.Draw(img)

    for elem in svg.elements():
        if not isinstance(elem, se.Shape):
            continue
        fill = color_to_rgba(elem.fill)
        if fill is None:
            continue
        r, g, b, a = fill
        canvas.path(shape_to_aggpath(elem, scale), aggdraw.Brush((r, g, b), opacity=a))

    canvas.flush()
    return img


def blue_rounded_rect(w: int, h: int) -> Image.Image:
    radius = max(1, round(min(w, h) * CORNER_RADIUS_RATIO))
    img = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    ImageDraw.Draw(img).rounded_rectangle((0, 0, w - 1, h - 1), radius=radius, fill=BLUE + (255,))
    return img


def render_icon(svg_content: str, size: int) -> Image.Image:
    """Blue rounded-rect background with the SVG glyph composited on top."""
    bg = blue_rounded_rect(size, size)
    glyph_size = round(size * ICON_GLYPH_RATIO)
    glyph = render_svg(svg_content, glyph_size * 4).resize((glyph_size, glyph_size), Image.LANCZOS)
    offset = (size - glyph_size) // 2
    bg.paste(glyph, (offset, offset), glyph)
    return bg


def render_banner(svg_content: str, font: ImageFont.FreeTypeFont) -> Image.Image:
    """Blue rounded-rect with the glyph and 'loom' text side by side."""
    SCALE = 4
    glyph = render_svg(svg_content, BANNER_LOGO_H * SCALE).resize(
        (BANNER_LOGO_H, BANNER_LOGO_H), Image.LANCZOS
    )

    dummy = ImageDraw.Draw(Image.new("RGBA", (1, 1)))
    bbox = dummy.textbbox((0, 0), "loom", font=font)
    tw, th = bbox[2] - bbox[0], bbox[3] - bbox[1]

    W = BANNER_LOGO_H + tw + BANNER_INNER_PAD * 2
    H = max(BANNER_LOGO_H, th) + BANNER_INNER_PAD * 2

    # Render background at 4x for smooth corners, then downscale
    hi_banner = blue_rounded_rect(W * SCALE, H * SCALE)
    banner = hi_banner.resize((W, H), Image.LANCZOS)

    draw = ImageDraw.Draw(banner)
    banner.paste(glyph, (BANNER_INNER_PAD, (H - BANNER_LOGO_H) // 2), glyph)
    ty = (H - BANNER_INNER_PAD) - th - bbox[1]
    draw.text((BANNER_INNER_PAD + BANNER_LOGO_H, ty), "loom", font=font, fill=(255, 255, 255, 255))

    return banner


if __name__ == "__main__":
    svg_content = SVG_PATH.read_text(encoding="utf-8")

    # ICO
    images = [render_icon(svg_content, s) for s in ICO_SIZES]
    images[0].save(ICO_PATH, format="ICO", append_images=images[1:])
    print(f"Written {ICO_PATH}")

    # PNGs — render at 4x then downscale for best quality
    for size in (32, 64):
        render_icon(svg_content, size * 4).resize((size, size), Image.LANCZOS).save(
            DIR / f"bloom{size}.png", format="PNG", optimize=True
        )
        print(f"Written {DIR / f'bloom{size}.png'}")

    # Banner
    font_path = next((p for p in BANNER_FONT_PATHS if Path(p).exists()), None)
    if font_path is None:
        raise RuntimeError(f"No suitable font found. Tried: {BANNER_FONT_PATHS}")
    font = ImageFont.truetype(font_path, size=BANNER_FONT_SIZE)

    banner = render_banner(svg_content, font)
    banner_path = DIR / "banner.png"
    banner.save(banner_path, format="PNG", optimize=True)
    print(f"Written {banner_path}")
