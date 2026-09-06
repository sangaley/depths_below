#!/usr/bin/env python3
"""Post-render pass: supersample down, then prove the tile actually tiles.

Runs on plain Pillow so it works before ImageMagick is installed; the
downscale and quantise steps move to `magick` once it is available.
"""

import sys
from PIL import Image


def downscale(src, dst, size):
    im = Image.open(src).convert("RGBA")
    im = im.resize((size, size), Image.LANCZOS)
    im.save(dst)
    return im


def tile_check(src, dst, n=3):
    """Montage NxN copies. Any seam the render introduced shows up here."""
    im = Image.open(src).convert("RGBA")
    w, h = im.size
    sheet = Image.new("RGBA", (w * n, h * n), (0, 0, 0, 0))
    for x in range(n):
        for y in range(n):
            sheet.paste(im, (x * w, y * h))
    sheet.save(dst)
    return sheet


def compare(paths, dst, scale=1, pad=8, bg=(18, 20, 26, 255)):
    """Side-by-side strip at a fixed height for honest comparison."""
    ims = [Image.open(p).convert("RGBA") for p in paths]
    h = max(i.height for i in ims) * scale
    ws = [int(i.width * (h / i.height)) for i in ims]
    sheet = Image.new("RGBA", (sum(ws) + pad * (len(ims) + 1), h + pad * 2), bg)
    x = pad
    for im, w in zip(ims, ws):
        sheet.paste(im.resize((w, h), Image.NEAREST), (x, pad), im.resize((w, h), Image.NEAREST))
        x += w + pad
    sheet.save(dst)
    return sheet


if __name__ == "__main__":
    cmd = sys.argv[1]
    if cmd == "down":
        downscale(sys.argv[2], sys.argv[3], int(sys.argv[4]))
    elif cmd == "tile":
        tile_check(sys.argv[2], sys.argv[3])
    elif cmd == "compare":
        compare(sys.argv[2:-1], sys.argv[-1])
    print("ok", cmd)
