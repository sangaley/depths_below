"""Hull tile, rendered in two treatments for an A/B.

Tiles are rendered as the centre of a 3x3 block so ambient occlusion and
light spill are continuous across the cell border. Rendering a lone tile
darkens its outer edge and produces a visible grid of seams once the engine
tiles it across a hull.

    Blender -b -P hull_tile.py -- restrained out.png
    Blender -b -P hull_tile.py -- full       out.png
"""

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from lib import (  # noqa: E402
    new_scene, metal, light_rig, ortho_camera, configure, render_to,
    box, cyl, argv_after_ddash,
)

CELL = 1.0
NEIGHBOURS = (-1, 0, 1)


def build_cell_restrained(cx, cy, mats):
    """Current silhouette, but with real bevels and contact AO."""
    # Raised armour plate, inset from the cell edge so neighbours read as
    # separate plates rather than one continuous sheet.
    box("plate", (0.94, 0.94, 0.08), (cx, cy, 0.08), mats["body"], bevel=0.014)
    # Corner rivets, matching the four in the existing sprite.
    for sx in (-1, 1):
        for sy in (-1, 1):
            cyl("rivet", 0.028, 0.03,
                (cx + sx * 0.36, cy + sy * 0.36, 0.125),
                mats["light"], bevel=0.008)


def build_cell_full(cx, cy, mats):
    """Heavier industrial read, but the plate stays ONE plate.

    An earlier pass quartered it; that made every tile read as four smaller
    tiles and fought the real grid. Detail here is added inside the plate
    instead of subdividing it.
    """
    # Outer plate with a recessed inner panel -- the step between them is
    # what gives the tile depth without breaking its silhouette.
    box("plate", (0.94, 0.94, 0.08), (cx, cy, 0.08), mats["body"], bevel=0.016)
    box("inner", (0.72, 0.72, 0.09), (cx, cy, 0.065), mats["dark"], bevel=0.010)

    # Heavy corner rivets and mid-edge fixings.
    for sx in (-1, 1):
        for sy in (-1, 1):
            cyl("rivet", 0.034, 0.036,
                (cx + sx * 0.395, cy + sy * 0.395, 0.128),
                mats["light"], bevel=0.010)
    for s in (-1, 1):
        cyl("bolt", 0.024, 0.030, (cx + s * 0.395, cy, 0.126), mats["highlight"])
        cyl("bolt", 0.024, 0.030, (cx, cy + s * 0.395, 0.126), mats["highlight"])

    # Raised cooling fins. Sinking them into the plate needed a boolean cut
    # and read as nothing once downscaled; standing proud of the surface they
    # catch the key light and hold their shape at 128px.
    for i in range(4):
        box("fin", (0.34, 0.028, 0.030),
            (cx - 0.13, cy - 0.20 + i * 0.058, 0.135), mats["light"])

    # Raised reinforcement rib across one corner: breaks the symmetry so a
    # tiled field does not read as wallpaper.
    rib = box("rib", (0.30, 0.045, 0.03), (cx + 0.20, cy + 0.20, 0.115),
              mats["light"], bevel=0.008)
    rib.rotation_euler = (0.0, 0.0, -0.7854)


def main():
    args = argv_after_ddash()
    variant = args[0] if args else "restrained"
    out = args[1] if len(args) > 1 else "/tmp/hull_%s.png" % variant
    res = int(args[2]) if len(args) > 2 else 512

    scene = new_scene()
    mats = {
        "recess": metal("recess", "recess", metallic=0.6, roughness=0.75),
        "dark": metal("dark", "dark", metallic=0.85, roughness=0.55),
        "body": metal("body", "body", metallic=0.85, roughness=0.50),
        "light": metal("light", "light", metallic=0.90, roughness=0.42),
        "highlight": metal("highlight", "highlight", metallic=0.92, roughness=0.35),
    }

    # Backing slab spans past the framed area so every rendered pixel is
    # opaque -- no semi-transparent edge pixels to seam against.
    box("backing", (3.6, 3.6, 0.08), (0.0, 0.0, 0.0), mats["recess"])

    builder = build_cell_full if variant == "full" else build_cell_restrained
    for ix in NEIGHBOURS:
        for iy in NEIGHBOURS:
            builder(ix * CELL, iy * CELL, mats)

    light_rig(scene)
    ortho_camera(scene, CELL)          # frame exactly the centre cell
    configure(scene, res)
    render_to(scene, out)


main()
