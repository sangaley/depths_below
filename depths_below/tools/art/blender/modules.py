"""Non-weapon module sprites, matching the weapon set's style and palette.

141 module types currently share 41 sprite files, so this regenerates the 41
rather than chasing 141 -- the pressing problem is that Blender-rendered
weapons now sit beside flat Pillow art on the same ship.

Colour follows the same rule as the weapons (ART_BRIEF: "Color = information"):
ION BLUE for power and energy, PLASMA ORANGE for thrust and heat, GREEN for
life support, AMBER for industrial, BRASS for stores.

    Blender -b -P modules.py -- small_reactor out.png 1024
"""

import math
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from lib import (  # noqa: E402
    new_scene, light_rig, configure, render_to, materials, armour_base, frame,
    box, cyl, bell, glow_disc, radial_glow, PLASMA_STOPS, argv_after_ddash,
)


def rods(m, cx, cy, r, n, mat, rr=0.030):
    """Ring of control-rod ports around a core."""
    for i in range(n):
        a = i * (math.pi * 2.0 / n)
        cyl("rod", rr, 0.05, (cx + math.cos(a) * r, cy + math.sin(a) * r, 0.132),
            mat, verts=14)


# ------------------------------------------------------------------- power
def build_small_reactor(m):
    """Fission pile: containment ring, control rods, ION BLUE core."""
    armour_base(m)
    cyl("shield", 0.300, 0.050, (0.0, 0.02, 0.096), m["dark"], bevel=0.014, verts=48)
    cyl("vessel", 0.235, 0.055, (0.0, 0.02, 0.116), m["light"], bevel=0.020, verts=48)
    cyl("well", 0.165, 0.050, (0.0, 0.02, 0.128), m["dark"], verts=48)
    glow_disc("core", 0.152, 0.046, (0.0, 0.02, 0.136), verts=56)
    rods(m, 0.0, 0.02, 0.200, 8, m["highlight"])
    # Coolant trunk across the top.
    box("trunk", (0.56, 0.055, 0.030), (0.0, 0.355, 0.098), m["dark"])
    for i in range(5):
        box("pipe", (0.070, 0.070, 0.024), (-0.20 + i * 0.10, 0.355, 0.120), m["light"])


def build_large_reactor_2x1(m):
    """Twin-vessel plant: two cores sharing one exchanger."""
    armour_base(m, w=1.94)
    for sx in (-1, 1):
        cyl("shield", 0.290, 0.050, (sx * 0.470, 0.02, 0.096), m["dark"],
            bevel=0.014, verts=48)
        cyl("vessel", 0.225, 0.055, (sx * 0.470, 0.02, 0.116), m["light"],
            bevel=0.020, verts=48)
        cyl("well", 0.155, 0.050, (sx * 0.470, 0.02, 0.128), m["dark"], verts=48)
        glow_disc("core%d" % sx, 0.142, 0.046, (sx * 0.470, 0.02, 0.136), verts=56)
        rods(m, sx * 0.470, 0.02, 0.192, 8, m["highlight"], rr=0.026)
    # Central heat exchanger tying the two vessels together.
    box("exch", (0.34, 0.62, 0.045), (0.0, 0.02, 0.098), m["dark"], bevel=0.010)
    for i in range(7):
        box("fin", (0.29, 0.032, 0.030), (0.0, -0.22 + i * 0.078, 0.124), m["light"])
    box("bus", (1.30, 0.048, 0.026), (0.0, 0.355, 0.098), m["dark"])
    for i in range(15):
        box("seg", (0.052, 0.058, 0.020), (-0.42 + i * 0.06, 0.355, 0.118),
            m["ion_lit"] if i % 2 else m["light"])


def build_large_reactor_3x3(m):
    """Fusion torus: magnetic confinement coils around a bright core. The
    biggest, brightest thing on any ship -- it should look like it."""
    armour_base(m, w=2.90, h=2.90)
    cyl("outer", 1.150, 0.050, (0.0, 0.0, 0.096), m["dark"], bevel=0.020, verts=64)
    cyl("ring", 0.960, 0.055, (0.0, 0.0, 0.118), m["light"], bevel=0.030, verts=64)
    cyl("cham", 0.720, 0.050, (0.0, 0.0, 0.150), m["dark"], verts=64)
    glow_disc("plasma", 0.585, 0.046, (0.0, 0.0, 0.170), verts=72, strength=4.6)
    # Confinement coils around the torus.
    for i in range(20):
        a = i * (math.pi * 2.0 / 20.0)
        c = box("coil", (0.180, 0.360, 0.060),
                (math.cos(a) * 0.960, math.sin(a) * 0.960, 0.140), m["highlight"],
                bevel=0.010)
        c.rotation_euler = (0.0, 0.0, a + 1.5708)
    # Corner support pylons.
    for sx in (-1, 1):
        for sy in (-1, 1):
            box("pylon", (0.30, 0.30, 0.050), (sx * 1.180, sy * 1.180, 0.100),
                m["dark"], bevel=0.014)
            cyl("cap", 0.090, 0.055, (sx * 1.180, sy * 1.180, 0.128),
                m["glow_ion"], verts=24)


def build_battery(m):
    """Cell bank: rows of cells with ION BLUE charge indicators."""
    armour_base(m)
    box("rack", (0.76, 0.66, 0.045), (0.0, 0.05, 0.096), m["dark"], bevel=0.012)
    for r in range(2):
        for c in range(3):
            x, y = -0.235 + c * 0.235, -0.115 + r * 0.325
            box("cell", (0.200, 0.275, 0.050), (x, y, 0.120), m["light"], bevel=0.010)
            # Charge bar down each cell.
            for k in range(3):
                box("bar", (0.140, 0.048, 0.022), (x, y - 0.075 + k * 0.075, 0.146),
                    m["glow_ion"] if k < 2 else m["ion"])
    box("bus", (0.72, 0.045, 0.026), (0.0, 0.372, 0.098), m["brass"])


# -------------------------------------------------------------- propulsion
def build_standard_engine(m):
    """Thruster. Art draws exhaust pointing DOWN (-Y) -- sprite_base_rotation
    reads placement rotation as the exhaust direction for engines."""
    armour_base(m)
    # Combustion chamber.
    box("chamber", (0.56, 0.42, 0.055), (0.0, 0.150, 0.100), m["dark"], bevel=0.014)
    for i in range(5):
        box("rib", (0.50, 0.036, 0.028), (0.0, 0.010 + i * 0.070, 0.128), m["light"])
    # Turbopump stacks either side.
    for sx in (-1, 1):
        cyl("pump", 0.085, 0.060, (sx * 0.325, 0.290, 0.110), m["light"],
            bevel=0.010, verts=24)
        cyl("inlet", 0.042, 0.05, (sx * 0.325, 0.290, 0.138), m["dark"], verts=16)
    # A real flared bell, not stacked boxes.
    bell("bell", 0.155, 0.330, 0.560, (0.0, -0.480, 0.078), m["light"])
    bell("bell_in", 0.120, 0.290, 0.540, (0.0, -0.478, 0.098), m["dark"])
    # Throat is a light SOURCE, hottest at the top of the bell.
    cyl("throat", 0.120, 0.040, (0.0, -0.235, 0.120), m["glow_plasma_hot"], verts=32)
    bell("plume", 0.105, 0.260, 0.480, (0.0, -0.500, 0.110),
         radial_glow("plume", PLASMA_STOPS, 5.0, 0.26))


def build_standard_engine_2x1(m):
    """Twin-nozzle engine on a 2x1 footprint. No overhang registered for this
    sprite, so the bells stay inside the canvas."""
    armour_base(m, w=1.94)
    box("chamber", (1.60, 0.34, 0.055), (0.0, 0.230, 0.100), m["dark"], bevel=0.014)
    for i in range(15):
        box("rib", (0.070, 0.28, 0.028), (-0.49 + i * 0.07, 0.230, 0.128), m["light"])
    for sx in (-1, 1):
        cyl("pump", 0.095, 0.060, (sx * 0.760, 0.230, 0.114), m["light"],
            bevel=0.010, verts=24)
        # Bell nozzle per side, opening downward.
        bell("bell", 0.140, 0.280, 0.400, (sx * 0.470, -0.270, 0.078), m["light"])
        bell("bell_in", 0.108, 0.245, 0.385, (sx * 0.470, -0.268, 0.098), m["dark"])
        cyl("throat", 0.105, 0.040, (sx * 0.470, -0.090, 0.120), m["glow_plasma_hot"],
            verts=32)
        bell("plume", 0.092, 0.215, 0.340, (sx * 0.470, -0.290, 0.110),
             radial_glow("plume%d" % sx, PLASMA_STOPS, 5.0, 0.215))


def build_silent_drive(m):
    """Stealth drive: baffled outlet, no visible plume. Deliberately the
    DARKEST module in the set -- it is the one that hides."""
    armour_base(m, hazard=False)
    box("shroud", (0.66, 0.50, 0.055), (0.0, 0.140, 0.098), m["dark"], bevel=0.016)
    # Acoustic baffling: staggered vanes instead of ribs.
    for i in range(6):
        off = 0.045 if i % 2 else -0.045
        box("vane", (0.44, 0.040, 0.026), (off, -0.010 + i * 0.062, 0.126), m["light"])
    for sx in (-1, 1):
        box("duct", (0.090, 0.44, 0.045), (sx * 0.355, 0.150, 0.098), m["dark"],
            bevel=0.008)
    # Diffuser outlet -- wide and COLD. The one drive with no glow at all;
    # that absence is the whole point of a stealth engine.
    bell("diff", 0.180, 0.330, 0.420, (0.0, -0.400, 0.078), m["dark"])
    bell("diff_in", 0.150, 0.295, 0.400, (0.0, -0.398, 0.096), m["recess"])
    for i in range(5):
        box("mesh", (0.052, 0.30, 0.022), (-0.14 + i * 0.07, -0.330, 0.112),
            m["recess"])


# -------------------------------------------------------------- structural
# hull_beam backs 23 module types (HullBeam, Bulkhead, ArmorPlate, HeatVent,
# PressureFrame...) and is the most repeated block on any ship. It must stay
# QUIET -- structure reads through shape, not detail. Anything busy here turns
# a large hull into visual noise, which is probably why the original is bare.
def build_hull_beam(m):
    """Reinforced structural bay: I-beam spine with corner gussets."""
    armour_base(m, hazard=False)
    box("spine", (0.20, 0.86, 0.045), (0.0, 0.0, 0.096), m["dark"], bevel=0.010)
    for sy in (-1, 1):
        box("flange", (0.74, 0.135, 0.045), (0.0, sy * 0.355, 0.096), m["dark"],
            bevel=0.010)
    for sx in (-1, 1):
        for sy in (-1, 1):
            g = box("gusset", (0.22, 0.075, 0.030), (sx * 0.245, sy * 0.245, 0.104),
                    m["light"])
            g.rotation_euler = (0.0, 0.0, -0.7854 * sx * sy)
    for sy in (-1, 1):
        for i in range(3):
            box("web", (0.135, 0.028, 0.024), (0.0, sy * (0.14 + i * 0.075), 0.122),
                m["light"])


def build_hull_beam_2x2(m):
    """Four-cell structural block: cross bracing across the full span."""
    armour_base(m, hazard=False, w=1.94, h=1.94)
    box("spine_v", (0.22, 1.86, 0.045), (0.0, 0.0, 0.096), m["dark"], bevel=0.010)
    box("spine_h", (1.86, 0.22, 0.045), (0.0, 0.0, 0.096), m["dark"], bevel=0.010)
    for sx in (-1, 1):
        for sy in (-1, 1):
            d = box("brace", (0.115, 1.05, 0.030), (sx * 0.470, sy * 0.470, 0.108),
                    m["light"])
            d.rotation_euler = (0.0, 0.0, -0.7854 * sx * sy)
    cyl("boss", 0.150, 0.050, (0.0, 0.0, 0.116), m["light"], bevel=0.012, verts=32)


def build_hull_beam_3x2(m):
    """Six-cell staggered armour: offset plates, no bright accents."""
    armour_base(m, hazard=False, w=2.90, h=1.94)
    for r in range(2):
        y = -0.470 + r * 0.940
        off = 0.235 if r else -0.235
        for i in range(3):
            box("plate", (0.86, 0.80, 0.040), (-0.940 + i * 0.940 + off, y, 0.098),
                m["dark"], bevel=0.014)
    box("spine_h", (2.82, 0.16, 0.040), (0.0, 0.0, 0.112), m["light"], bevel=0.008)
    for i in range(9):
        box("stud", (0.070, 0.070, 0.024), (-1.12 + i * 0.28, 0.0, 0.130), m["light"])


# ------------------------------------------------------------ life support
# GREEN accent: breathable atmosphere, the "you are alive" colour.
def build_life_support(m):
    """Atmosphere processor: circulation fan over filter beds."""
    armour_base(m)
    box("housing", (0.78, 0.62, 0.045), (0.0, 0.055, 0.096), m["dark"], bevel=0.012)
    cyl("fanring", 0.230, 0.050, (0.0, 0.180, 0.116), m["light"], bevel=0.012, verts=40)
    cyl("fanwell", 0.180, 0.048, (0.0, 0.180, 0.128), m["recess"], verts=40)
    for i in range(6):
        a = i * (math.pi / 3.0)
        bl = box("blade", (0.055, 0.165, 0.020), (math.cos(a) * 0.092,
                 0.180 + math.sin(a) * 0.092, 0.138), m["light"])
        bl.rotation_euler = (0.0, 0.0, a + 0.5)
    cyl("hub", 0.052, 0.046, (0.0, 0.180, 0.148), m["utility_lit"], verts=24)
    for i in range(3):
        box("bed", (0.68, 0.075, 0.030), (0.0, -0.135 - i * 0.090, 0.122),
            m["utility"] if i == 1 else m["light"])


def build_oxygen_scrubber(m):
    """O2 plant: pressure bottles over a distribution manifold."""
    armour_base(m)
    for i, x in enumerate((-0.265, 0.0, 0.265)):
        cyl("bottle", 0.115, 0.055, (x, 0.135, 0.112), m["light"], bevel=0.012,
            verts=32)
        cyl("valve", 0.048, 0.050, (x, 0.135, 0.136), m["utility"], verts=20)
        box("gauge", (0.030, 0.150, 0.022), (x, -0.055, 0.130),
            m["utility_lit"] if i != 2 else m["dark"])
    box("manifold", (0.74, 0.090, 0.035), (0.0, -0.215, 0.104), m["dark"], bevel=0.008)
    for i in range(5):
        cyl("port", 0.032, 0.040, (-0.24 + i * 0.12, -0.215, 0.126), m["light"],
            verts=16)


def build_oxygen_scrubber_2x1(m):
    """Twin-column oxygenator on a 2x1 footprint."""
    armour_base(m, w=1.94)
    for sx in (-1, 1):
        box("col", (0.72, 0.62, 0.045), (sx * 0.470, 0.045, 0.096), m["dark"],
            bevel=0.012)
        for i in range(3):
            cyl("bottle", 0.098, 0.055, (sx * 0.470 - 0.215 + i * 0.215, 0.150,
                0.114), m["light"], bevel=0.010, verts=28)
            cyl("valve", 0.040, 0.050, (sx * 0.470 - 0.215 + i * 0.215, 0.150,
                0.138), m["utility"], verts=18)
        box("bed", (0.62, 0.085, 0.030), (sx * 0.470, -0.140, 0.122), m["utility_lit"])
    box("cross", (0.30, 0.075, 0.030), (0.0, 0.045, 0.104), m["light"])


# --------------------------------------------------------------- detection
# Active emitters read ION BLUE, passive listening reads GREEN. That split
# tells a player at a glance whether a block is broadcasting their position.
def build_sonar_array(m):
    """Active array: sweep arm over a phased element grid."""
    armour_base(m)
    cyl("dish", 0.320, 0.050, (0.0, 0.02, 0.096), m["dark"], bevel=0.014, verts=48)
    for r in range(4):
        for c in range(4):
            x, y = -0.165 + c * 0.110, 0.02 - 0.165 + r * 0.110
            if x * x + (y - 0.02) ** 2 < 0.062:
                box("elem", (0.070, 0.070, 0.024), (x, y, 0.116), m["light"])
    arm = box("arm", (0.048, 0.560, 0.028), (0.0, 0.02, 0.132), m["ion"])
    arm.rotation_euler = (0.0, 0.0, -0.6)
    cyl("pivot", 0.062, 0.050, (0.0, 0.02, 0.144), m["ion_lit"], verts=24)


def build_passive_sonar(m):
    """Hydrophone bank: it only listens -- no emitter, GREEN read."""
    armour_base(m)
    box("bed", (0.78, 0.66, 0.045), (0.0, 0.045, 0.096), m["dark"], bevel=0.012)
    for r in range(3):
        for c in range(4):
            cyl("phone", 0.058, 0.050, (-0.255 + c * 0.170, -0.145 + r * 0.195,
                0.118), m["light"], bevel=0.008, verts=20)
            cyl("mem", 0.028, 0.046, (-0.255 + c * 0.170, -0.145 + r * 0.195,
                0.136), m["utility"], verts=16)
    box("pre", (0.30, 0.070, 0.028), (0.0, 0.345, 0.104), m["utility_lit"])


def build_passive_sonar_2x1(m):
    """Long-baseline array -- a wider baseline gives a better bearing, so it
    spreads across both cells."""
    armour_base(m, w=1.94)
    box("bed", (1.78, 0.60, 0.045), (0.0, 0.040, 0.096), m["dark"], bevel=0.012)
    for c in range(10):
        x = -0.765 + c * 0.170
        cyl("phone", 0.058, 0.050, (x, 0.140, 0.118), m["light"], bevel=0.008, verts=20)
        cyl("mem", 0.028, 0.046, (x, 0.140, 0.136), m["utility"], verts=16)
        box("lead", (0.026, 0.150, 0.022), (x, -0.085, 0.118), m["light"])
    box("pre", (1.62, 0.070, 0.028), (0.0, -0.215, 0.122), m["utility_lit"])


def build_depth_sensor(m):
    """Compact sensor head: graduated aperture around a glowing eye."""
    armour_base(m)
    cyl("ring", 0.290, 0.050, (0.0, 0.03, 0.096), m["dark"], bevel=0.014, verts=48)
    cyl("iris", 0.205, 0.048, (0.0, 0.03, 0.112), m["light"], verts=40)
    cyl("well", 0.140, 0.046, (0.0, 0.03, 0.124), m["recess"], verts=40)
    glow_disc("eye", 0.088, 0.044, (0.0, 0.03, 0.136), verts=32, strength=4.0)
    for i in range(12):
        a = i * (math.pi / 6.0)
        t = box("tick", (0.026, 0.055, 0.022), (math.cos(a) * 0.245,
                0.03 + math.sin(a) * 0.245, 0.116),
                m["ion"] if i % 3 == 0 else m["light"])
        t.rotation_euler = (0.0, 0.0, a + 1.5708)


# ----------------------------------------------------------------- storage
# BRASS accent: cargo, ammunition, anything you count rather than consume.
def crates(m, cx, cy, cols, rows, cw=0.215, ch=0.215):
    for r in range(rows):
        for c in range(cols):
            x = cx - (cols - 1) * cw * 0.5 + c * cw
            y = cy - (rows - 1) * ch * 0.5 + r * ch
            box("crate", (cw * 0.86, ch * 0.86, 0.045), (x, y, 0.112), m["light"],
                bevel=0.010)
            box("strap", (cw * 0.86, 0.030, 0.024), (x, y, 0.138),
                m["brass"] if (r + c) % 2 == 0 else m["dark"])


def build_cargo_hold(m):
    """Stacked crates with tie-down straps."""
    armour_base(m)
    box("floor", (0.80, 0.66, 0.040), (0.0, 0.045, 0.094), m["dark"], bevel=0.012)
    crates(m, 0.0, 0.045, 3, 2, 0.245, 0.265)


def build_cargo_hold_2x1(m):
    armour_base(m, w=1.94)
    box("floor", (1.80, 0.66, 0.040), (0.0, 0.045, 0.094), m["dark"], bevel=0.012)
    crates(m, 0.0, 0.045, 7, 2, 0.245, 0.265)


def build_cargo_hold_2x2(m):
    armour_base(m, w=1.94, h=1.94)
    box("floor", (1.80, 1.66, 0.040), (0.0, 0.0, 0.094), m["dark"], bevel=0.012)
    crates(m, 0.0, 0.0, 7, 6, 0.245, 0.265)


def build_ballast_tank(m):
    """Fluid tank with a sight glass showing level -- fuel, ballast, coolant."""
    armour_base(m)
    cyl("tank", 0.320, 0.055, (0.0, 0.02, 0.100), m["dark"], bevel=0.018, verts=48)
    cyl("shell", 0.265, 0.050, (0.0, 0.02, 0.120), m["light"], bevel=0.020, verts=48)
    # Sight glass: filled to about two thirds.
    box("glass", (0.105, 0.420, 0.030), (0.0, 0.02, 0.140), m["recess"])
    box("level", (0.085, 0.270, 0.024), (0.0, -0.055, 0.150), m["amber"])
    for sy in (-1, 1):
        cyl("port", 0.062, 0.050, (0.0, 0.02 + sy * 0.300, 0.126), m["light"], verts=24)
    for sx in (-1, 1):
        box("band", (0.055, 0.520, 0.026), (sx * 0.215, 0.02, 0.132), m["dark"])


# -------------------------------------------------------------------- crew
# Crew spaces are the WARM blocks: brass fittings, soft light, no hazard
# striping. They should read as somewhere a person lives, not machinery.
def bunks(m, cx, cy, cols, rows):
    for r in range(rows):
        for c in range(cols):
            x = cx - (cols - 1) * 0.235 + c * 0.235
            y = cy - (rows - 1) * 0.300 + r * 0.300
            box("bunk", (0.195, 0.255, 0.040), (x, y, 0.110), m["light"], bevel=0.010)
            box("mat", (0.150, 0.180, 0.024), (x, y - 0.020, 0.134), m["dark"])
            box("pillow", (0.130, 0.055, 0.022), (x, y + 0.082, 0.136), m["brass"])


def build_basic_quarters(m):
    """Crew berths: two bunks and a locker."""
    armour_base(m, hazard=False)
    box("deck", (0.80, 0.72, 0.038), (0.0, 0.0, 0.094), m["dark"], bevel=0.012)
    bunks(m, 0.0, 0.075, 2, 1)
    box("locker", (0.66, 0.135, 0.040), (0.0, -0.290, 0.110), m["light"], bevel=0.010)
    for i in range(4):
        cyl("handle", 0.022, 0.036, (-0.21 + i * 0.14, -0.290, 0.134), m["brass"],
            verts=14)


def build_basic_quarters_2x1(m):
    armour_base(m, hazard=False, w=1.94)
    box("deck", (1.80, 0.72, 0.038), (0.0, 0.0, 0.094), m["dark"], bevel=0.012)
    bunks(m, 0.0, 0.075, 6, 1)
    box("locker", (1.70, 0.135, 0.040), (0.0, -0.290, 0.110), m["light"], bevel=0.010)


def build_basic_quarters_2x2(m):
    """Galley/mess: tables rather than bunks."""
    armour_base(m, hazard=False, w=1.94, h=1.94)
    box("deck", (1.80, 1.80, 0.038), (0.0, 0.0, 0.094), m["dark"], bevel=0.012)
    for sx in (-1, 1):
        for sy in (-1, 1):
            cyl("table", 0.215, 0.045, (sx * 0.450, sy * 0.450, 0.112), m["light"],
                bevel=0.012, verts=32)
            for i in range(4):
                a = i * (math.pi / 2.0) + 0.78
                cyl("stool", 0.062, 0.040, (sx * 0.450 + math.cos(a) * 0.320,
                    sy * 0.450 + math.sin(a) * 0.320, 0.108), m["brass"], verts=16)
    box("counter", (1.70, 0.150, 0.045), (0.0, 0.0, 0.112), m["dark"], bevel=0.010)


def build_basic_quarters_3x3(m):
    """Wellness hub: a green space at the centre of the ship."""
    armour_base(m, hazard=False, w=2.90, h=2.90)
    box("deck", (2.76, 2.76, 0.038), (0.0, 0.0, 0.094), m["dark"], bevel=0.014)
    cyl("garden", 0.900, 0.048, (0.0, 0.0, 0.108), m["light"], bevel=0.020, verts=48)
    cyl("bed", 0.700, 0.046, (0.0, 0.0, 0.124), m["utility"], verts=48)
    for i in range(8):
        a = i * (math.pi / 4.0)
        cyl("plant", 0.115, 0.050, (math.cos(a) * 0.430, math.sin(a) * 0.430, 0.140),
            m["utility_lit"], verts=20)
    cyl("tree", 0.180, 0.055, (0.0, 0.0, 0.146), m["utility_lit"], verts=32)
    for sx in (-1, 1):
        for sy in (-1, 1):
            bunks(m, sx * 1.080, sy * 1.020, 1, 2)


def build_medical_bay(m):
    """Treatment berth with a monitor. Green cross, no hazard striping."""
    armour_base(m, hazard=False)
    box("deck", (0.80, 0.72, 0.038), (0.0, 0.0, 0.094), m["dark"], bevel=0.012)
    box("bed", (0.360, 0.560, 0.045), (-0.135, -0.020, 0.112), m["light"], bevel=0.012)
    box("sheet", (0.290, 0.400, 0.024), (-0.135, -0.060, 0.138), m["highlight"])
    box("cross_v", (0.075, 0.230, 0.026), (0.235, 0.130, 0.130), m["utility_lit"])
    box("cross_h", (0.230, 0.075, 0.026), (0.235, 0.130, 0.130), m["utility_lit"])
    box("mon", (0.250, 0.190, 0.040), (0.235, -0.220, 0.110), m["dark"], bevel=0.010)
    for i in range(3):
        box("trace", (0.190, 0.026, 0.022), (0.235, -0.280 + i * 0.058, 0.134),
            m["utility"])


def build_medical_bay_3x2(m):
    """Surgical suite: table under a lamp array, instrument carts either side."""
    armour_base(m, hazard=False, w=2.90, h=1.94)
    box("deck", (2.76, 1.80, 0.038), (0.0, 0.0, 0.094), m["dark"], bevel=0.014)
    box("table", (0.520, 1.10, 0.048), (0.0, -0.060, 0.112), m["light"], bevel=0.014)
    box("sheet", (0.420, 0.860, 0.026), (0.0, -0.090, 0.140), m["highlight"])
    # Surgical lamp head.
    for i in range(6):
        a = i * (math.pi / 3.0)
        glow_disc("lamp%d" % i, 0.105, 0.044, (math.cos(a) * 0.230,
                  0.640 + math.sin(a) * 0.230, 0.140), stops=None, strength=3.0,
                  verts=24)
    cyl("lamphub", 0.115, 0.046, (0.0, 0.640, 0.140), m["light"], verts=28)
    for sx in (-1, 1):
        box("cart", (0.420, 1.20, 0.045), (sx * 1.020, -0.060, 0.110), m["dark"],
            bevel=0.012)
        for i in range(5):
            box("tray", (0.340, 0.150, 0.024), (sx * 1.020, -0.560 + i * 0.250,
                0.134), m["utility"] if i % 2 else m["light"])


def build_research_lab(m):
    """Specimen vault: sealed tanks with something alive inside."""
    armour_base(m)
    box("frame", (0.80, 0.68, 0.040), (0.0, 0.035, 0.094), m["dark"], bevel=0.012)
    for i, x in enumerate((-0.235, 0.0, 0.235)):
        cyl("tank", 0.105, 0.055, (x, 0.115, 0.114), m["light"], bevel=0.010, verts=32)
        glow_disc("spec%d" % i, 0.070, 0.046, (x, 0.115, 0.140),
                  stops=[(0.0, "#0e2418"), (0.5, "utility"), (1.0, "utility_lit")],
                  strength=3.2, verts=24)
    box("bench", (0.72, 0.150, 0.038), (0.0, -0.225, 0.110), m["light"], bevel=0.008)
    for i in range(5):
        cyl("vial", 0.026, 0.040, (-0.24 + i * 0.12, -0.225, 0.132), m["utility_lit"],
            verts=14)


def build_research_lab_2x1(m):
    """Containment lab: one large cell plus analysis benches."""
    armour_base(m, w=1.94)
    box("frame", (1.80, 0.68, 0.040), (0.0, 0.035, 0.094), m["dark"], bevel=0.012)
    cyl("cell", 0.300, 0.055, (-0.470, 0.035, 0.112), m["light"], bevel=0.016, verts=44)
    glow_disc("subject", 0.205, 0.046, (-0.470, 0.035, 0.140),
              stops=[(0.0, "#0e2418"), (0.5, "utility"), (1.0, "utility_lit")],
              strength=3.4, verts=40)
    for i in range(4):
        a = i * (math.pi / 2.0) + 0.78
        box("clamp", (0.075, 0.150, 0.030), (-0.470 + math.cos(a) * 0.320,
            0.035 + math.sin(a) * 0.320, 0.126), m["dark"])
    for r in range(2):
        box("bench", (0.80, 0.185, 0.038), (0.520, 0.215 - r * 0.360, 0.110),
            m["light"], bevel=0.008)
        for i in range(5):
            cyl("vial", 0.026, 0.040, (0.250 + i * 0.135, 0.215 - r * 0.360, 0.132),
                m["utility_lit"] if i % 2 else m["dark"], verts=14)


# ---------------------------------------------------------------- utility
def build_repair_station(m):
    """Workbench with a tool gantry. AMBER: industrial, not weaponry."""
    armour_base(m)
    box("bench", (0.78, 0.32, 0.045), (0.0, -0.180, 0.098), m["dark"], bevel=0.012)
    for i in range(5):
        box("tool", (0.075, 0.230, 0.026), (-0.26 + i * 0.13, -0.180, 0.124),
            m["amber"] if i % 2 else m["light"])
    # Overhead gantry rail with a travelling head.
    box("rail", (0.80, 0.070, 0.030), (0.0, 0.300, 0.100), m["light"])
    box("head", (0.185, 0.185, 0.045), (-0.140, 0.190, 0.114), m["dark"], bevel=0.010)
    cyl("torch", 0.048, 0.048, (-0.140, 0.120, 0.140), m["amber"], verts=20)
    for sx in (-1, 1):
        box("post", (0.065, 0.230, 0.030), (sx * 0.335, 0.185, 0.100), m["dark"])


def build_repair_station_2x1(m):
    """Drone bay: cradles plus the gantry."""
    armour_base(m, w=1.94)
    box("rail", (1.80, 0.070, 0.030), (0.0, 0.320, 0.100), m["light"])
    for i, x in enumerate((-0.620, 0.0, 0.620)):
        box("cradle", (0.480, 0.480, 0.042), (x, -0.045, 0.098), m["dark"], bevel=0.012)
        cyl("drone", 0.145, 0.050, (x, -0.045, 0.120), m["light"], bevel=0.012, verts=28)
        cyl("eye", 0.058, 0.046, (x, -0.045, 0.142), m["amber"] if i != 1 else
            m["dark"], verts=20)
        for sx in (-1, 1):
            box("arm", (0.055, 0.320, 0.026), (x + sx * 0.190, -0.045, 0.126),
                m["light"])
    box("head", (0.185, 0.185, 0.045), (0.310, 0.215, 0.114), m["dark"], bevel=0.010)


def build_navigation(m):
    """Helm console: a screen you read, so the screen is the whole design."""
    armour_base(m)
    box("desk", (0.82, 0.62, 0.045), (0.0, -0.040, 0.096), m["dark"], bevel=0.014)
    box("bezel", (0.68, 0.400, 0.040), (0.0, 0.075, 0.116), m["light"], bevel=0.010)
    glow_disc("screen", 0.175, 0.038, (0.0, 0.075, 0.140), strength=2.6, verts=40)
    # Scan lines over the display.
    for i in range(5):
        box("line", (0.560, 0.020, 0.020), (0.0, -0.055 + i * 0.065, 0.148),
            m["dark"])
    for i in range(6):
        box("key", (0.085, 0.075, 0.024), (-0.265 + i * 0.106, -0.245, 0.124),
            m["ion"] if i % 3 == 0 else m["light"])


def build_navigation_2x1(m):
    """Combat core: paired displays and a processor stack."""
    armour_base(m, w=1.94)
    box("desk", (1.82, 0.62, 0.045), (0.0, -0.040, 0.096), m["dark"], bevel=0.014)
    for sx in (-1, 1):
        box("bezel", (0.62, 0.380, 0.040), (sx * 0.540, 0.085, 0.116), m["light"],
            bevel=0.010)
        glow_disc("scr%d" % sx, 0.160, 0.038, (sx * 0.540, 0.085, 0.140),
                  strength=2.6, verts=36)
        for i in range(4):
            box("line", (0.520, 0.020, 0.020), (sx * 0.540, -0.030 + i * 0.062, 0.148),
                m["dark"])
    for i in range(7):
        box("blade", (0.055, 0.330, 0.030), (-0.180 + i * 0.060, -0.040, 0.120),
            m["ion"] if i % 2 else m["light"])


def build_navigation_3x2(m):
    """Bridge wing: a bank of stations along the forward edge."""
    armour_base(m, w=2.90, h=1.94)
    box("desk", (2.76, 0.72, 0.045), (0.0, 0.420, 0.096), m["dark"], bevel=0.014)
    for i in range(4):
        x = -1.020 + i * 0.680
        box("bezel", (0.560, 0.440, 0.040), (x, 0.460, 0.116), m["light"], bevel=0.010)
        glow_disc("scr%d" % i, 0.170, 0.038, (x, 0.460, 0.140), strength=2.4, verts=36)
    # Plot table aft of the consoles.
    cyl("plot", 0.520, 0.048, (0.0, -0.420, 0.106), m["dark"], bevel=0.016, verts=48)
    glow_disc("holo", 0.360, 0.044, (0.0, -0.420, 0.128), strength=2.2, verts=44)
    for i in range(8):
        a = i * (math.pi / 4.0)
        cyl("seat", 0.090, 0.042, (math.cos(a) * 0.760, -0.420 + math.sin(a) * 0.420,
            0.104), m["light"], verts=18)


def build_floodlight(m):
    """Lamp. The one module whose job is emitting visible light, so the lens
    is the brightest thing in the whole sprite set."""
    armour_base(m)
    box("yoke", (0.72, 0.24, 0.045), (0.0, -0.250, 0.098), m["dark"], bevel=0.012)
    cyl("housing", 0.310, 0.055, (0.0, 0.075, 0.104), m["dark"], bevel=0.018, verts=48)
    cyl("reflector", 0.250, 0.050, (0.0, 0.075, 0.124), m["highlight"], verts=48)
    glow_disc("lens", 0.190, 0.046, (0.0, 0.075, 0.142),
              stops=[(0.0, "#3a3320"), (0.45, "amber"), (0.8, "#ffe9b0"),
                     (1.0, "#ffffff")], strength=8.0, verts=44)
    for sx in (-1, 1):
        cyl("pivot", 0.055, 0.055, (sx * 0.310, 0.075, 0.116), m["light"], verts=20)


def build_docking_port(m):
    """Airlock: clamp ring around a sealed hatch."""
    armour_base(m)
    cyl("collar", 0.335, 0.050, (0.0, 0.0, 0.096), m["dark"], bevel=0.016, verts=48)
    cyl("ring", 0.270, 0.052, (0.0, 0.0, 0.114), m["light"], bevel=0.014, verts=48)
    cyl("hatch", 0.195, 0.048, (0.0, 0.0, 0.130), m["dark"], verts=48)
    # Radial dogs -- what actually locks a hatch.
    for i in range(8):
        a = i * (math.pi / 4.0)
        d = box("dog", (0.080, 0.145, 0.030), (math.cos(a) * 0.235,
                math.sin(a) * 0.235, 0.134), m["highlight"])
        d.rotation_euler = (0.0, 0.0, a + 1.5708)
    cyl("wheel", 0.075, 0.046, (0.0, 0.0, 0.146), m["brass"], verts=24)


def build_docking_port_3x3(m):
    """Docking hub: a berth big enough to take a ship."""
    armour_base(m, w=2.90, h=2.90)
    cyl("collar", 1.180, 0.050, (0.0, 0.0, 0.096), m["dark"], bevel=0.020, verts=64)
    cyl("apron", 1.000, 0.048, (0.0, 0.0, 0.110), m["light"], verts=64)
    cyl("well", 0.780, 0.046, (0.0, 0.0, 0.122), m["dark"], verts=64)
    cyl("hatch", 0.520, 0.046, (0.0, 0.0, 0.134), m["light"], verts=64)
    box("split", (0.045, 1.04, 0.026), (0.0, 0.0, 0.150), m["recess"])
    for i in range(12):
        a = i * (math.pi / 6.0)
        d = box("dog", (0.130, 0.230, 0.032), (math.cos(a) * 0.640,
                math.sin(a) * 0.640, 0.140), m["highlight"])
        d.rotation_euler = (0.0, 0.0, a + 1.5708)
    # Approach lights around the rim.
    for i in range(8):
        a = i * (math.pi / 4.0) + 0.39
        glow_disc("marker%d" % i, 0.070, 0.044, (math.cos(a) * 1.090,
                  math.sin(a) * 1.090, 0.126),
                  stops=[(0.0, "#0e2418"), (0.6, "utility"), (1.0, "utility_lit")],
                  strength=4.0, verts=20)


def build_salvage_arm(m):
    """Articulated arm reaching PAST the block, claw open at the tip."""
    armour_base(m)
    cyl("turret", 0.215, 0.055, (0.0, -0.080, 0.104), m["dark"], bevel=0.014, verts=40)
    cyl("shoulder", 0.135, 0.050, (0.0, -0.080, 0.126), m["light"], verts=32)
    # Upper and lower arm sections.
    box("upper", (0.150, 0.560, 0.050), (0.0, 0.240, 0.118), m["light"], bevel=0.010)
    for i in range(5):
        box("joint", (0.175, 0.030, 0.028), (0.0, 0.030 + i * 0.105, 0.146), m["dark"])
    cyl("elbow", 0.105, 0.050, (0.0, 0.520, 0.130), m["dark"], bevel=0.010, verts=28)
    box("fore", (0.115, 0.330, 0.046), (0.0, 0.690, 0.124), m["light"], bevel=0.008)
    # Claw: two jaws, open.
    for sx in (-1, 1):
        j = box("jaw", (0.070, 0.250, 0.044), (sx * 0.105, 0.930, 0.124), m["amber"],
                bevel=0.010)
        j.rotation_euler = (0.0, 0.0, sx * 0.42)
    cyl("wrist", 0.090, 0.048, (0.0, 0.845, 0.132), m["dark"], verts=24)


def build_mine_layer(m):
    """Dispenser rack: mines on a rail, ready to drop. DANGER RED."""
    armour_base(m)
    box("rack", (0.80, 0.58, 0.042), (0.0, 0.075, 0.096), m["dark"], bevel=0.012)
    for i in range(3):
        x = -0.245 + i * 0.245
        cyl("mine", 0.098, 0.052, (x, 0.155, 0.116), m["light"], bevel=0.012, verts=28)
        # Contact spines, the universal "this is a mine" read.
        for k in range(6):
            a = k * (math.pi / 3.0)
            sp = box("spine", (0.030, 0.070, 0.024), (x + math.cos(a) * 0.118,
                     0.155 + math.sin(a) * 0.118, 0.128), m["danger"])
            sp.rotation_euler = (0.0, 0.0, a + 1.5708)
        cyl("fuse", 0.040, 0.046, (x, 0.155, 0.140), m["danger_lit"], verts=18)
    box("rail", (0.74, 0.075, 0.030), (0.0, -0.200, 0.106), m["light"])
    for i in range(4):
        box("stop", (0.055, 0.100, 0.024), (-0.225 + i * 0.15, -0.200, 0.126),
            m["danger"])


BUILDERS = {
    "small_reactor": (build_small_reactor, dict()),
    "large_reactor_2x1": (build_large_reactor_2x1, dict(cells_w=2)),
    "large_reactor_3x3": (build_large_reactor_3x3, dict(cells_w=3, cells_h=3)),
    "battery": (build_battery, dict()),
    "standard_engine": (build_standard_engine, dict(overhang=30.0, protrude=-1.0)),
    "standard_engine_2x1": (build_standard_engine_2x1, dict(cells_w=2)),
    "silent_drive": (build_silent_drive, dict(overhang=24.0, protrude=-1.0)),
    "hull_beam": (build_hull_beam, dict()),
    "hull_beam_2x2": (build_hull_beam_2x2, dict(cells_w=2, cells_h=2)),
    "hull_beam_3x2": (build_hull_beam_3x2, dict(cells_w=3, cells_h=2)),
    "life_support": (build_life_support, dict()),
    "oxygen_scrubber": (build_oxygen_scrubber, dict()),
    "oxygen_scrubber_2x1": (build_oxygen_scrubber_2x1, dict(cells_w=2)),
    "sonar_array": (build_sonar_array, dict()),
    "passive_sonar": (build_passive_sonar, dict()),
    "passive_sonar_2x1": (build_passive_sonar_2x1, dict(cells_w=2)),
    "depth_sensor": (build_depth_sensor, dict()),
    "cargo_hold": (build_cargo_hold, dict()),
    "cargo_hold_2x1": (build_cargo_hold_2x1, dict(cells_w=2)),
    "cargo_hold_2x2": (build_cargo_hold_2x2, dict(cells_w=2, cells_h=2)),
    "ballast_tank": (build_ballast_tank, dict()),
    "basic_quarters": (build_basic_quarters, dict()),
    "basic_quarters_2x1": (build_basic_quarters_2x1, dict(cells_w=2)),
    "basic_quarters_2x2": (build_basic_quarters_2x2, dict(cells_w=2, cells_h=2)),
    "basic_quarters_3x3": (build_basic_quarters_3x3, dict(cells_w=3, cells_h=3)),
    "medical_bay": (build_medical_bay, dict()),
    "medical_bay_3x2": (build_medical_bay_3x2, dict(cells_w=3, cells_h=2)),
    "research_lab": (build_research_lab, dict()),
    "research_lab_2x1": (build_research_lab_2x1, dict(cells_w=2)),
    "repair_station": (build_repair_station, dict()),
    "repair_station_2x1": (build_repair_station_2x1, dict(cells_w=2)),
    "navigation": (build_navigation, dict()),
    "navigation_2x1": (build_navigation_2x1, dict(cells_w=2)),
    "navigation_3x2": (build_navigation_3x2, dict(cells_w=3, cells_h=2)),
    "floodlight": (build_floodlight, dict()),
    "docking_port": (build_docking_port, dict()),
    "docking_port_3x3": (build_docking_port_3x3, dict(cells_w=3, cells_h=3)),
    "salvage_arm": (build_salvage_arm, dict(overhang=34.0, protrude=1.0)),
    "mine_layer": (build_mine_layer, dict()),
}


def main():
    args = argv_after_ddash()
    which = args[0] if args else "small_reactor"
    out = args[1] if len(args) > 1 else "/tmp/%s.png" % which
    res = int(args[2]) if len(args) > 2 else 1024

    scene = new_scene()
    m = materials()
    builder, fkw = BUILDERS[which]
    builder(m)
    light_rig(scene)
    configure(scene, res, samples=200)
    frame(scene, res, **fkw)
    render_to(scene, out)


main()
