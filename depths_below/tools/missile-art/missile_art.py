"""Missile sprites, nose to the right (+x, the way a missile flies), one per
seeker head so a salvo reads at a glance: heat-seeker (red glass), radar
(white radome), command link (dark nose, green band), unguided (plain
steel), and the old one-block launchers' rocket. Drawn large and shrunk, so
the shading survives at a few pixels long.

Writes assets/sprites/missiles/*.png. Run from depths_below/:
python3 tools/missile-art/missile_art.py
"""
import os
from PIL import Image, ImageDraw

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
OUT = os.path.join(ROOT, "assets/sprites/missiles")
W, H, S = 128, 32, 4  # final size and the supersample
OUTLINE = (18, 20, 26, 255)


def shade(c, k):
    return tuple(max(0, min(255, int(v * k))) for v in c[:3]) + (255,)


def cylinder(d, x0, x1, y0, y1, base):
    """A round body: light along the top, dark underneath."""
    h = y1 - y0
    for i in range(h):
        t = i / max(h - 1, 1)
        k = 1.35 - 0.75 * t if t < 0.35 else 1.1 - 0.55 * t
        d.line([(x0, y0 + i), (x1, y0 + i)], fill=shade(base, k))


def missile(body, nose, band, glow=None, stripes=True):
    im = Image.new("RGBA", (W * S, H * S), (0, 0, 0, 0))
    d = ImageDraw.Draw(im)
    s = lambda v: int(v * S)
    cy = H / 2
    top, bot = cy - 5.5, cy + 5.5
    # Fins, tail and mid: behind the body.
    for sign in (-1, 1):
        d.polygon([(s(20), s(cy + sign * 4)), (s(40), s(cy + sign * 4)), (s(26), s(cy + sign * 14)), (s(16), s(cy + sign * 14))], fill=OUTLINE)
        d.polygon([(s(21), s(cy + sign * 4)), (s(38), s(cy + sign * 4)), (s(26), s(cy + sign * 13)), (s(17.5), s(cy + sign * 13))], fill=shade(body, 0.7))
        d.polygon([(s(66), s(cy + sign * 4)), (s(76), s(cy + sign * 4)), (s(68), s(cy + sign * 8.5))], fill=shade(body, 0.6))
    # Engine bell.
    d.polygon([(s(9), s(cy - 4.5)), (s(18), s(cy - 3.5)), (s(18), s(cy + 3.5)), (s(9), s(cy + 4.5))], fill=OUTLINE)
    d.polygon([(s(10), s(cy - 3.5)), (s(18), s(cy - 2.8)), (s(18), s(cy + 2.8)), (s(10), s(cy + 3.5))], fill=(62, 60, 64, 255))
    d.rectangle([s(9), s(cy - 2.4), s(11), s(cy + 2.4)], fill=(255, 150, 60, 255))
    # Body.
    d.rectangle([s(17), s(top - 1), s(101), s(bot + 1)], fill=OUTLINE)
    cylinder(d, s(18), s(100), s(top), s(bot), body)
    # Panel seams and the warning ring.
    for x in (44, 58, 84):
        d.line([(s(x), s(top)), (s(x), s(bot))], fill=shade(body, 0.62), width=S)
    if stripes:
        for i, x in enumerate(range(48, 56, 2)):
            d.rectangle([s(x), s(top), s(x + 1), s(bot)], fill=(214, 168, 40, 255) if i % 2 == 0 else (30, 30, 30, 255))
    # Seeker band.
    cylinder(d, s(90), s(100), s(top), s(bot), band)
    # Nose: an ogive to a point.
    pts = []
    for i in range(13):
        t = i / 12
        x = 100 + 24 * t
        r = 5.5 * (1 - t ** 1.6)
        pts.append((x, r))
    up = [(s(x), s(cy - r - 1)) for x, r in pts] + [(s(125.5), s(cy))]
    dn = [(s(x), s(cy + r + 1)) for x, r in reversed(pts)]
    d.polygon(up + dn, fill=OUTLINE)
    for i in range(int(11 * S)):
        y = s(top) + i
        rel = (y - s(cy)) / s(5.5)
        xs = [x for x, r in pts if abs(rel) * 5.5 <= r]
        if xs:
            d.line([(s(100), y), (s(max(xs)), y)], fill=shade(nose, 1.3 - 0.6 * (rel + 1) / 2))
    if glow:
        d.ellipse([s(108), s(cy - 2.5), s(114), s(cy + 0.5)], fill=glow)
    return im.resize((W, H), Image.LANCZOS)


os.makedirs(OUT, exist_ok=True)
STEEL = (150, 156, 166)
OLIVE = (112, 120, 96)
looks = {
    "missile_heat": missile(STEEL, (120, 40, 36), (150, 46, 40), glow=(255, 120, 90, 255)),
    "missile_radar": missile(STEEL, (226, 226, 218), (190, 190, 184)),
    "missile_command": missile(STEEL, (60, 64, 72), (60, 170, 96), glow=(120, 255, 150, 255)),
    "missile_dumb": missile(STEEL, (128, 132, 140), (110, 114, 122), stripes=False),
    "rocket": missile(OLIVE, (160, 70, 50), (150, 140, 110)),
}
for name, im in looks.items():
    im.save(os.path.join(OUT, name + ".png"))
    print("wrote", name)
sheet = Image.new("RGBA", (W * 2 + 20, (H + 10) * len(looks) * 2), (24, 26, 34, 255))
for i, im in enumerate(looks.values()):
    sheet.alpha_composite(im.resize((W * 2, H * 2), Image.NEAREST), (10, i * (H + 10) * 2 + 10))
sheet.save(os.path.join(os.environ.get("SHEET_DIR", "/tmp"), "missile_sheet.png"))
