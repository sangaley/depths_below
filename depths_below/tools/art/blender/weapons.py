"""Weapon module sprites -- one distinct silhouette per weapon.

Fourteen weapons currently share four sprites (see sprite_map.rs): a Laser
renders identically to a MiningDrill. Since combat targeting is per-block
right-click, a player cannot pick a target they cannot identify.

Every weapon here shares a common armour base so they read as one family,
then carries a distinct mechanism on top so they read as different guns.

    Blender -b -P weapons.py -- railgun out.png 1024
"""

import math
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from lib import (  # noqa: E402
    new_scene, metal, light_rig, ortho_camera, configure, render_to,
    box, cyl, cone, argv_after_ddash,
)

CELL = 1.0

# Launchers whose art protrudes forward past the block, on a 378x541 canvas
# matching the existing torpedo_tube convention.
OVERHANG = {"heavy_missile", "guided_missile", "cluster_rocket"}
OVERHANG_H = 541.0

# Modules whose registry footprint is 2 cells wide.
WIDE = {"railgun_2x1"}


def materials():
    return {
        "recess": metal("recess", "recess", metallic=0.10, roughness=0.80),
        "dark": metal("dark", "dark", metallic=0.15, roughness=0.60),
        "body": metal("body", "body_warm", metallic=0.15, roughness=0.55),
        "light": metal("light", "light", metallic=0.20, roughness=0.45),
        "highlight": metal("highlight", "highlight", metallic=0.25, roughness=0.38),
        "gold": metal("gold", "gold", metallic=0.35, roughness=0.45),
        # Damage-type accents.
        "brass": metal("brass", "brass", metallic=0.55, roughness=0.42),
        "brass_lit": metal("brass_lit", "brass_lit", metallic=0.55, roughness=0.35),
        "ion": metal("ion", "ion", metallic=0.30, roughness=0.35),
        "ion_lit": metal("ion_lit", "ion_lit", metallic=0.25, roughness=0.28),
        "plasma": metal("plasma", "plasma", metallic=0.25, roughness=0.40),
        "plasma_lit": metal("plasma_lit", "plasma_lit", metallic=0.20, roughness=0.32),
        "danger": metal("danger", "danger", metallic=0.20, roughness=0.48),
        "danger_lit": metal("danger_lit", "danger_lit", metallic=0.20, roughness=0.40),
        "utility": metal("utility", "utility", metallic=0.28, roughness=0.40),
        "utility_lit": metal("utility_lit", "utility_lit", metallic=0.24, roughness=0.33),
        "amber": metal("amber", "amber", metallic=0.45, roughness=0.42),
    }


def armour_base(m, hazard=True, w=0.94):
    """Shared chassis every weapon sits on: bevelled plate, corner bolts,
    and a hazard strip along the mounting edge."""
    box("plate", (w, 0.94, 0.08), (0.0, 0.0, 0.04), m["body"], bevel=0.013)

    bx = w / 2.0 - 0.085
    for sx in (-1, 1):
        for sy in (-1, 1):
            cyl("bolt", 0.021, 0.024, (sx * bx, sy * 0.385, 0.086),
                m["light"], bevel=0.006)

    if hazard:
        # Alternating gold/dark blocks. Gold is the only saturated colour in
        # the palette and ART_BRIEF reserves it for caution striping.
        hw = w - 0.22
        box("hazbed", (hw, 0.070, 0.018), (0.0, -0.375, 0.083), m["recess"])
        # Chevrons sized so their rotated bounding box stays inside the bed --
        # an earlier pass had them spilling off the plate and burying a bolt.
        n = int(hw / 0.060) + 1
        for i in range(n):
            x = -(n - 1) * 0.030 + i * 0.060
            ch = box("haz", (0.024, 0.062, 0.010), (x, -0.375, 0.094), m["gold"])
            ch.rotation_euler = (0.0, 0.0, -0.7854)   # 45deg = hazard tape


def turret_ring(m, radius=0.22):
    """Rotating collar + recessed bore. The barrel is a separate sprite that
    pivots here, so the base must read as a mount, not a gun."""
    cyl("collar", radius, 0.07, (0.0, 0.0, 0.105), m["light"], bevel=0.012, verts=32)
    cyl("bore", radius * 0.62, 0.06, (0.0, 0.0, 0.125), m["recess"], verts=32)



# ---------------------------------------------------------------- barrel base
def barrel_hub(m, tail=0.18, hub_r=0.165):
    """Pivot hub plus a tail behind it.

    Barrels rotate about the image centre and are meant to slide back on
    firing (combat/recoil.rs currently only kicks the ship, but the art
    should not have to be redone when it animates). The tail means a barrel
    can retract without exposing a gap at the breech.
    """
    cyl("hub", hub_r, 0.07, (0.0, 0.0, 0.05), m["light"], bevel=0.010, verts=48)
    box("tail", (hub_r * 1.5, tail, 0.06), (0.0, -tail * 0.5, 0.048), m["dark"],
        bevel=0.008)


def emitter_base(m, hazard=True):
    """Shared plate for non-turret weapons."""
    armour_base(m, hazard=hazard)


# ------------------------------------------------------------------ railgun
def build_railgun(m):
    """Railgun: pulsed-power capacitor banks driving a rail assembly.

    A real railgun is NOT a cannon -- its bore is the rectangular gap between
    two facing rails and the shot is driven by a capacitor store rather than
    a propellant charge. Accent is ION BLUE: this is an electromagnetic gun.
    """
    armour_base(m)
    cyl("race", 0.185, 0.055, (0.0, 0.03, 0.098), m["light"], verts=64)
    for i in range(20):
        a = i * (math.pi * 2.0 / 20.0)
        t = box("tooth", (0.030, 0.020, 0.030),
                (math.cos(a) * 0.196, 0.03 + math.sin(a) * 0.196, 0.098), m["dark"])
        t.rotation_euler = (0.0, 0.0, a)
    cyl("race_in", 0.115, 0.05, (0.0, 0.03, 0.115), m["recess"], verts=48)

    for sx in (-1, 1):
        box("bank", (0.155, 0.60, 0.035), (sx * 0.315, 0.045, 0.097), m["dark"],
            bevel=0.008)
        for i in range(3):
            y = -0.16 + i * 0.205
            cyl("can", 0.062, 0.055, (sx * 0.315, y, 0.128), m["light"],
                bevel=0.006, verts=32)
            cyl("charge", 0.034, 0.05, (sx * 0.315, y, 0.146), m["ion"], verts=24)

    box("bus", (0.50, 0.038, 0.026), (0.0, 0.345, 0.098), m["dark"])
    for i in range(7):
        box("busseg", (0.045, 0.048, 0.020), (-0.18 + i * 0.06, 0.345, 0.116),
            m["ion_lit"] if i % 2 else m["light"])
    for sx in (-1, 1):
        box("lead", (0.048, 0.15, 0.022), (sx * 0.20, 0.268, 0.096), m["light"])


def build_railgun_barrel(m):
    """Two continuous rails flanking an open rectangular bore.

    Regular transverse banding renders as a screw thread; a railgun's identity
    is LONGITUDINAL, so length lines dominate and cross-banding is limited to
    a few structural collars.
    """
    barrel_hub(m)
    box("breech", (0.310, 0.200, 0.085), (0.0, 0.155, 0.065), m["dark"], bevel=0.014)
    box("body", (0.150, 0.560, 0.075), (0.0, 0.525, 0.062), m["dark"], bevel=0.008)
    for sx in (-1, 1):
        box("rail", (0.038, 0.560, 0.030), (sx * 0.052, 0.525, 0.092), m["ion"])
    box("bore", (0.058, 0.560, 0.024), (0.0, 0.525, 0.086), m["recess"])
    for y in (0.300, 0.520, 0.740):
        box("collar", (0.180, 0.030, 0.096), (0.0, y, 0.062), m["light"], bevel=0.006)
        box("collar_c", (0.058, 0.032, 0.026), (0.0, y, 0.100), m["recess"])
    box("muzzle", (0.210, 0.075, 0.098), (0.0, 0.838, 0.058), m["light"], bevel=0.014)
    box("mslot", (0.058, 0.050, 0.05), (0.0, 0.838, 0.110), m["ion_lit"])


def build_railgun_2x1(m):
    """Railgun at its actual registry footprint (2x1).

    ModuleType::Railgun is the ONLY 2x1 weapon; every other weapon is 1x1.
    Twice the plate means twice the capacitor store, which is the honest read
    for the game's heaviest kinetic gun.
    """
    armour_base(m, w=1.94)
    cyl("race", 0.185, 0.055, (0.0, 0.03, 0.098), m["light"], verts=64)
    for i in range(20):
        a = i * (math.pi * 2.0 / 20.0)
        t = box("tooth", (0.030, 0.020, 0.030),
                (math.cos(a) * 0.196, 0.03 + math.sin(a) * 0.196, 0.098), m["dark"])
        t.rotation_euler = (0.0, 0.0, a)
    cyl("race_in", 0.115, 0.05, (0.0, 0.03, 0.115), m["recess"], verts=48)

    # Four capacitor cans per side instead of three, pushed out to the wings.
    for sx in (-1, 1):
        box("bank", (0.185, 0.72, 0.035), (sx * 0.360, 0.02, 0.097), m["dark"],
            bevel=0.008)
        for i in range(4):
            y = -0.235 + i * 0.170
            cyl("can", 0.068, 0.055, (sx * 0.360, y, 0.128), m["light"],
                bevel=0.006, verts=32)
            cyl("charge", 0.038, 0.05, (sx * 0.360, y, 0.146), m["ion"], verts=24)
        # Outboard cooling stack in the extra cell.
        box("rad", (0.30, 0.62, 0.040), (sx * 0.720, 0.02, 0.098), m["dark"],
            bevel=0.008)
        for i in range(7):
            box("fin", (0.26, 0.030, 0.030), (sx * 0.720, -0.235 + i * 0.078, 0.126),
                m["light"])

    box("bus", (0.98, 0.038, 0.026), (0.0, 0.365, 0.098), m["dark"])
    for i in range(13):
        box("busseg", (0.045, 0.048, 0.020), (-0.36 + i * 0.06, 0.365, 0.116),
            m["ion_lit"] if i % 2 else m["light"])


# ------------------------------------------------------------------- cannon
def build_cannon(m):
    """Propellant autocannon. Accent is BRASS -- it eats physical shells, and
    the exposed belt says so at a glance."""
    armour_base(m)
    cyl("race", 0.185, 0.055, (0.0, 0.02, 0.098), m["light"], verts=64)
    cyl("race_in", 0.120, 0.05, (0.0, 0.02, 0.115), m["recess"], verts=48)
    for sx in (-1, 1):
        cyl("recoil", 0.070, 0.42, (sx * 0.30, 0.06, 0.115), m["dark"],
            bevel=0.010, verts=24).rotation_euler = (1.5708, 0.0, 0.0)
        cyl("rod", 0.028, 0.30, (sx * 0.30, 0.30, 0.130), m["highlight"], verts=16
            ).rotation_euler = (1.5708, 0.0, 0.0)
    box("feed", (0.30, 0.16, 0.045), (0.0, -0.20, 0.100), m["dark"], bevel=0.010)
    for i in range(5):
        cyl("round", 0.026, 0.105, (-0.10 + i * 0.05, -0.20, 0.128),
            m["brass_lit"] if i % 2 else m["brass"], verts=14
            ).rotation_euler = (1.5708, 0.0, 0.0)


def build_cannon_barrel(m):
    """Short, thick, ROUND bore with a slotted muzzle brake -- the round bore
    is exactly what separates a cannon from the railgun."""
    barrel_hub(m, tail=0.20, hub_r=0.175)
    box("breech", (0.320, 0.220, 0.090), (0.0, 0.150, 0.066), m["dark"], bevel=0.016)
    cyl("tube", 0.082, 0.520, (0.0, 0.480, 0.070), m["light"], bevel=0.010, verts=32
        ).rotation_euler = (1.5708, 0.0, 0.0)
    box("brake", (0.215, 0.150, 0.105), (0.0, 0.790, 0.062), m["dark"], bevel=0.014)
    for i in range(3):
        for sx in (-1, 1):
            box("vent", (0.055, 0.028, 0.045), (sx * 0.075, 0.735 + i * 0.045, 0.104),
                m["recess"])
    cyl("bore", 0.040, 0.06, (0.0, 0.868, 0.078), m["recess"], verts=24
        ).rotation_euler = (1.5708, 0.0, 0.0)
    box("band", (0.180, 0.030, 0.050), (0.0, 0.245, 0.082), m["brass"])


# ------------------------------------------------------------------ coilgun
def build_coilgun(m):
    """Sequential electromagnet stages, no rails. ION BLUE like the railgun --
    both electromagnetic -- but the stage drivers ring the mount instead of
    sitting in two banks."""
    armour_base(m)
    cyl("race", 0.180, 0.055, (0.0, 0.02, 0.098), m["light"], verts=64)
    cyl("race_in", 0.115, 0.05, (0.0, 0.02, 0.115), m["recess"], verts=48)
    for i in range(7):
        a = -2.36 + i * 0.655
        x, y = math.cos(a) * 0.335, 0.02 + math.sin(a) * 0.335
        d = box("drv", (0.105, 0.075, 0.045), (x, y, 0.100), m["dark"], bevel=0.006)
        d.rotation_euler = (0.0, 0.0, a + 1.5708)
        c = cyl("coil", 0.030, 0.055, (x, y, 0.128), m["ion_lit"], verts=20)
        c.rotation_euler = (0.0, 0.0, a)
    box("bus", (0.46, 0.036, 0.024), (0.0, 0.375, 0.096), m["ion"])


def build_coilgun_barrel(m):
    """Discrete coil stages down a slim tube."""
    barrel_hub(m, tail=0.16, hub_r=0.150)
    box("breech", (0.260, 0.170, 0.075), (0.0, 0.135, 0.062), m["dark"], bevel=0.012)
    cyl("tube", 0.048, 0.600, (0.0, 0.500, 0.068), m["dark"], verts=24
        ).rotation_euler = (1.5708, 0.0, 0.0)
    for i in range(9):
        y = 0.245 + i * 0.068
        cyl("coil", 0.088, 0.046, (0.0, y, 0.076), m["light"], bevel=0.008, verts=24
            ).rotation_euler = (1.5708, 0.0, 0.0)
        cyl("wind", 0.092, 0.014, (0.0, y, 0.076), m["ion"], verts=24
            ).rotation_euler = (1.5708, 0.0, 0.0)
    cyl("muzz", 0.062, 0.055, (0.0, 0.865, 0.072), m["light"], verts=24
        ).rotation_euler = (1.5708, 0.0, 0.0)
    cyl("bore", 0.028, 0.06, (0.0, 0.878, 0.078), m["ion_lit"], verts=20
        ).rotation_euler = (1.5708, 0.0, 0.0)


# ------------------------------------------------------------------ gatling
def build_gatling(m):
    """Rotary autocannon: the drum magazine is the identifying mass, and it is
    full of visible BRASS."""
    armour_base(m)
    cyl("race", 0.170, 0.05, (0.0, 0.06, 0.098), m["light"], verts=64)
    cyl("race_in", 0.108, 0.05, (0.0, 0.06, 0.114), m["recess"], verts=48)
    cyl("drum", 0.275, 0.060, (0.0, -0.16, 0.105), m["dark"], bevel=0.012, verts=48)
    for i in range(10):
        a = i * (math.pi * 2.0 / 10.0)
        cyl("cell", 0.038, 0.05, (math.cos(a) * 0.185, -0.16 + math.sin(a) * 0.185,
            0.132), m["brass_lit"] if i % 2 == 0 else m["brass"], verts=16)
    cyl("hubcap", 0.075, 0.05, (0.0, -0.16, 0.136), m["light"], verts=32)


def build_gatling_barrel(m):
    """Six barrels in a rotary cluster."""
    barrel_hub(m, tail=0.14, hub_r=0.155)
    box("breech", (0.270, 0.175, 0.078), (0.0, 0.130, 0.062), m["dark"], bevel=0.012)
    for i in range(6):
        x = -0.105 + i * 0.042
        cyl("bbl", 0.019, 0.560, (x, 0.500, 0.084), m["light"], verts=12
            ).rotation_euler = (1.5708, 0.0, 0.0)
    for y in (0.320, 0.520, 0.715):
        box("clamp", (0.245, 0.034, 0.062), (0.0, y, 0.076), m["dark"], bevel=0.006)
    box("front", (0.250, 0.052, 0.070), (0.0, 0.812, 0.074), m["light"], bevel=0.010)
    box("band", (0.250, 0.026, 0.048), (0.0, 0.230, 0.082), m["brass"])


# -------------------------------------------------------------- mining drill
def build_mining_drill(m):
    """Not a gun: a boring head. AMBER industrial accent, no ammunition."""
    armour_base(m)
    cyl("race", 0.180, 0.055, (0.0, 0.05, 0.098), m["light"], verts=64)
    cyl("race_in", 0.112, 0.05, (0.0, 0.05, 0.115), m["recess"], verts=48)
    box("motor", (0.46, 0.20, 0.055), (0.0, -0.20, 0.105), m["dark"], bevel=0.012)
    for i in range(6):
        box("cool", (0.028, 0.155, 0.030), (-0.155 + i * 0.062, -0.20, 0.135),
            m["amber"] if i % 2 else m["light"])
    for sx in (-1, 1):
        box("chute", (0.105, 0.130, 0.040), (sx * 0.335, 0.10, 0.100), m["dark"],
            bevel=0.008)
        box("slot", (0.060, 0.085, 0.026), (sx * 0.335, 0.10, 0.128), m["recess"])


def build_mining_drill_barrel(m):
    """Auger helix -- here a thread read is CORRECT, it is what an auger is."""
    barrel_hub(m, tail=0.15, hub_r=0.160)
    box("chuck", (0.290, 0.190, 0.085), (0.0, 0.145, 0.064), m["dark"], bevel=0.014)
    cyl("shaft", 0.055, 0.560, (0.0, 0.490, 0.068), m["dark"], verts=20
        ).rotation_euler = (1.5708, 0.0, 0.0)
    for i in range(13):
        y = 0.255 + i * 0.048
        f = box("flight", (0.185, 0.030, 0.026), (0.0, y, 0.080),
                m["amber"] if i % 3 == 0 else m["light"])
        f.rotation_euler = (0.0, 0.0, i * 0.42)
    for i in range(4):
        a = i * (math.pi / 2.0) + 0.4
        t = box("tooth", (0.055, 0.048, 0.040),
                (math.cos(a) * 0.070, 0.845 + math.sin(a) * 0.070, 0.078), m["amber"])
        t.rotation_euler = (0.0, 0.0, a)
    cyl("tip", 0.062, 0.050, (0.0, 0.845, 0.086), m["highlight"], bevel=0.008, verts=24)


# ------------------------------------------------------------------- beams
def build_laser(m):
    """Beam emitter FIRING FORWARD (+Y), not at the camera.

    Top-down: the aperture must sit at the leading edge of the housing, seen
    end-on. A lens drawn flat on the top face would be firing out of the
    screen. Radiators fill the rear -- beam weapons dump heat.
    """
    emitter_base(m)
    # Emitter snout at the front, aperture facing +Y.
    box("snout", (0.34, 0.30, 0.075), (0.0, 0.235, 0.100), m["dark"], bevel=0.014)
    box("lens", (0.20, 0.055, 0.055), (0.0, 0.375, 0.118), m["plasma"])
    box("core", (0.10, 0.030, 0.045), (0.0, 0.386, 0.132), m["plasma_lit"])
    # Focusing collars stepping back from the aperture.
    for i, w in enumerate((0.28, 0.24, 0.20)):
        box("collar", (w, 0.030, 0.065), (0.0, 0.300 - i * 0.062, 0.112), m["light"])
    # Radiator stack across the rear.
    box("radbed", (0.66, 0.36, 0.040), (0.0, -0.185, 0.096), m["dark"], bevel=0.008)
    for i in range(7):
        box("fin", (0.62, 0.026, 0.046), (0.0, -0.325 + i * 0.047, 0.122), m["light"])


def build_plasma_caster(m):
    """Containment sphere feeding a forward nozzle. PLASMA ORANGE = thermal."""
    emitter_base(m)
    # Containment vessel sits at the rear.
    cyl("cradle", 0.265, 0.050, (0.0, -0.13, 0.095), m["dark"], bevel=0.012, verts=48)
    cyl("s1", 0.200, 0.060, (0.0, -0.13, 0.118), m["light"], bevel=0.030, verts=48)
    cyl("s2", 0.135, 0.055, (0.0, -0.13, 0.140), m["plasma"], bevel=0.030, verts=48)
    cyl("core", 0.070, 0.050, (0.0, -0.13, 0.158), m["plasma_lit"], bevel=0.020, verts=32)
    # Feed throat running forward to the nozzle.
    box("throat", (0.145, 0.34, 0.055), (0.0, 0.155, 0.100), m["dark"], bevel=0.010)
    # Flared nozzle, aperture facing +Y.
    box("nozzle", (0.30, 0.13, 0.080), (0.0, 0.355, 0.104), m["light"], bevel=0.016)
    box("mouth", (0.165, 0.050, 0.050), (0.0, 0.398, 0.130), m["plasma_lit"])
    for sx in (-1, 1):
        cyl("vent", 0.050, 0.065, (sx * 0.315, -0.13, 0.108), m["dark"],
            bevel=0.008, verts=20)
        cyl("vhole", 0.026, 0.05, (sx * 0.315, -0.13, 0.134), m["plasma"], verts=16)


def build_ion_disruptor(m):
    """Forked electrodes with an open arc gap facing forward. ION BLUE."""
    emitter_base(m)
    box("base", (0.44, 0.24, 0.055), (0.0, -0.15, 0.100), m["dark"], bevel=0.012)
    for i in range(4):
        box("coil", (0.36, 0.026, 0.030), (0.0, -0.245 + i * 0.058, 0.130), m["ion"])
    for sx in (-1, 1):
        box("prong", (0.075, 0.400, 0.055), (sx * 0.175, 0.13, 0.105), m["light"],
            bevel=0.010)
        box("tip", (0.090, 0.075, 0.065), (sx * 0.175, 0.315, 0.115), m["dark"],
            bevel=0.012)
        cyl("elec", 0.032, 0.055, (sx * 0.175, 0.335, 0.142), m["ion_lit"], verts=20)
    # The arc gap is deliberately empty -- that void IS the weapon.
    box("insul", (0.130, 0.075, 0.030), (0.0, 0.055, 0.108), m["recess"])


def build_emp_pulse(m):
    """Omnidirectional coil -- the ONE weapon that legitimately reads flat from
    above, because its pulse radiates in every direction rather than firing."""
    emitter_base(m)
    cyl("outer", 0.310, 0.050, (0.0, 0.04, 0.096), m["dark"], bevel=0.014, verts=64)
    cyl("coil", 0.255, 0.055, (0.0, 0.04, 0.118), m["light"], bevel=0.020, verts=64)
    # Recessed core, NOT a hole. An earlier pass left a stark black void here.
    cyl("well", 0.165, 0.050, (0.0, 0.04, 0.104), m["dark"], verts=64)
    cyl("core", 0.105, 0.048, (0.0, 0.04, 0.112), m["ion"], verts=48)
    cyl("spark", 0.048, 0.046, (0.0, 0.04, 0.126), m["ion_lit"], verts=32)
    for i in range(16):
        a = i * (math.pi * 2.0 / 16.0)
        w = box("wind", (0.048, 0.115, 0.030),
                (math.cos(a) * 0.207, 0.04 + math.sin(a) * 0.207, 0.140),
                m["ion_lit"] if i % 2 else m["ion"])
        w.rotation_euler = (0.0, 0.0, a + 1.5708)
    for sx in (-1, 1):
        cyl("post", 0.045, 0.075, (sx * 0.355, -0.20, 0.110), m["highlight"], verts=16)


def build_tractor_beam(m):
    """Parabolic reflector OPENING FORWARD, seen edge-on from above as a curved
    bracket. A dish drawn flat on the top face would be pulling straight up out
    of the play plane. UTILITY GREEN -- non-lethal."""
    emitter_base(m)
    # Reflector arc: segments on a circle, opening toward +Y.
    for i in range(11):
        a = -1.15 + i * 0.23
        x, y = math.sin(a) * 0.335, 0.10 - math.cos(a) * 0.335
        seg = box("refl", (0.085, 0.055, 0.070), (x, y, 0.100), m["light"], bevel=0.006)
        seg.rotation_euler = (0.0, 0.0, a)
        seg2 = box("refl_i", (0.050, 0.026, 0.030), (x * 0.86, 0.10 - (0.10 - y) * 0.86,
                   0.132), m["utility"])
        seg2.rotation_euler = (0.0, 0.0, a)
    # Emitter at the focus, projecting forward.
    box("mast", (0.070, 0.185, 0.050), (0.0, -0.145, 0.104), m["dark"], bevel=0.008)
    cyl("emit", 0.078, 0.060, (0.0, 0.010, 0.126), m["dark"], bevel=0.012, verts=32)
    cyl("lens", 0.046, 0.050, (0.0, 0.010, 0.150), m["utility_lit"], verts=24)
    # Power trunk at the rear.
    box("trunk", (0.42, 0.10, 0.042), (0.0, -0.315, 0.098), m["dark"], bevel=0.008)
    for i in range(5):
        box("cell", (0.058, 0.062, 0.026), (-0.14 + i * 0.07, -0.315, 0.122),
            m["utility"])


# ---------------------------------------------------------------- launchers
# These protrude forward onto a 378x541 canvas. The tubes lie IN the play
# plane pointing +Y with their mouths at the leading edge -- NOT circular
# holes on the top face, which would launch toward the camera.
def build_heavy_missile(m):
    """One heavy tube, warhead nose protruding forward. DANGER RED."""
    emitter_base(m)
    # Launch rails either side of the tube.
    for sx in (-1, 1):
        box("rail", (0.085, 0.62, 0.060), (sx * 0.275, 0.16, 0.102), m["dark"],
            bevel=0.010)
        for i in range(5):
            box("rung", (0.060, 0.030, 0.024), (sx * 0.275, -0.08 + i * 0.115, 0.134),
                m["light"])
    # Tube channel.
    box("channel", (0.335, 0.66, 0.050), (0.0, 0.175, 0.098), m["recess"])
    box("tube", (0.290, 0.64, 0.060), (0.0, 0.170, 0.104), m["dark"], bevel=0.010)
    # The missile: body inside the tube, nose out past the block.
    cyl("body", 0.105, 0.66, (0.0, 0.315, 0.132), m["light"], verts=28
        ).rotation_euler = (1.5708, 0.0, 0.0)
    for i in range(4):
        box("band", (0.225, 0.026, 0.030), (0.0, 0.125 + i * 0.145, 0.156), m["danger"])
    cone("nose", 0.105, 0.235, (0.0, 0.765, 0.132), m["danger_lit"], verts=28)
    # Blast deflector at the base.
    box("deflect", (0.50, 0.085, 0.045), (0.0, -0.235, 0.100), m["dark"], bevel=0.008)


def build_guided_missile(m):
    """Four slim tubes side by side, noses forward. DANGER RED."""
    emitter_base(m)
    box("block", (0.86, 0.60, 0.050), (0.0, 0.14, 0.096), m["dark"], bevel=0.012)
    for i in range(4):
        x = -0.285 + i * 0.19
        box("cradle", (0.155, 0.58, 0.055), (x, 0.145, 0.104), m["recess"])
        cyl("body", 0.062, 0.60, (x, 0.235, 0.128), m["light"], verts=20
            ).rotation_euler = (1.5708, 0.0, 0.0)
        for j in range(3):
            box("band", (0.135, 0.022, 0.026), (x, 0.070 + j * 0.170, 0.148),
                m["danger"])
        cone("nose", 0.062, 0.165, (x, 0.615, 0.128), m["danger_lit"], verts=20)
        # Fins, visible from above as small tabs.
        for sx in (-1, 1):
            box("fin", (0.045, 0.075, 0.020), (x + sx * 0.075, 0.010, 0.140),
                m["light"])
    box("spine", (0.86, 0.075, 0.040), (0.0, -0.245, 0.100), m["dark"], bevel=0.008)


def build_cluster_rocket(m):
    """Dense pod of many small rockets, all pointing forward. DANGER RED."""
    emitter_base(m)
    box("pod", (0.88, 0.56, 0.050), (0.0, 0.13, 0.096), m["dark"], bevel=0.012)
    # Two staggered ranks so it reads as "many" without becoming mush.
    for rank, (ycell, n, y0) in enumerate(((0.055, 6, 0.44), (0.245, 5, 0.60))):
        for i in range(n):
            x = -((n - 1) * 0.5) * 0.155 + i * 0.155
            cyl("body", 0.045, 0.34, (x, ycell + 0.09, 0.124 + rank * 0.006),
                m["light"], verts=16).rotation_euler = (1.5708, 0.0, 0.0)
            box("tube", (0.115, 0.34, 0.048), (x, ycell + 0.06, 0.100), m["recess"])
            cone("nose", 0.045, 0.115, (x, y0, 0.124 + rank * 0.006),
                 m["danger_lit"] if i % 2 else m["danger"], verts=16)
    box("mani", (0.88, 0.085, 0.042), (0.0, -0.235, 0.100), m["dark"], bevel=0.008)
    for i in range(6):
        box("valve", (0.070, 0.050, 0.024), (-0.28 + i * 0.112, -0.235, 0.124),
            m["danger"])


def build_ammo_autoloader(m):
    """Support, not a weapon: carousel and belt feed, all BRASS. No aperture,
    because it does not fire anything."""
    emitter_base(m)
    cyl("carousel", 0.235, 0.055, (0.0, 0.14, 0.098), m["dark"], bevel=0.012, verts=48)
    for i in range(8):
        a = i * (math.pi / 4.0)
        cyl("shell", 0.042, 0.055, (math.cos(a) * 0.150, 0.14 + math.sin(a) * 0.150,
            0.126), m["brass_lit"] if i % 2 == 0 else m["brass"], verts=16)
    cyl("spindle", 0.058, 0.050, (0.0, 0.14, 0.132), m["light"], verts=24)
    box("track", (0.24, 0.42, 0.040), (0.0, -0.20, 0.098), m["dark"], bevel=0.008)
    for i in range(6):
        box("link", (0.175, 0.045, 0.028), (0.0, -0.36 + i * 0.062, 0.124),
            m["brass"] if i % 2 else m["light"])


def build_point_defense(m):
    """Point-defence mount: compact, fast-traversing, BRASS-fed. Small and
    plain on purpose -- it is the cheap block you bolt on everywhere."""
    armour_base(m)
    cyl("race", 0.200, 0.055, (0.0, 0.03, 0.098), m["light"], verts=64)
    for i in range(16):
        a = i * (math.pi * 2.0 / 16.0)
        t = box("tooth", (0.028, 0.020, 0.030), (math.cos(a) * 0.210,
                0.03 + math.sin(a) * 0.210, 0.098), m["dark"])
        t.rotation_euler = (0.0, 0.0, a)
    cyl("race_in", 0.128, 0.05, (0.0, 0.03, 0.116), m["recess"], verts=48)
    # Ready-round drums either side of the mount.
    for sx in (-1, 1):
        cyl("drum", 0.115, 0.055, (sx * 0.320, -0.075, 0.110), m["dark"],
            bevel=0.012, verts=28)
        for i in range(6):
            a = i * (math.pi / 3.0)
            cyl("rnd", 0.026, 0.05, (sx * 0.320 + math.cos(a) * 0.062,
                -0.075 + math.sin(a) * 0.062, 0.136),
                m["brass_lit"] if i % 2 else m["brass"], verts=12)
    box("sensor", (0.30, 0.085, 0.030), (0.0, 0.355, 0.100), m["ion"])


BUILDERS = {
    "point_defense": build_point_defense,
    "railgun": build_railgun,
    "railgun_2x1": build_railgun_2x1,
    "railgun_barrel": build_railgun_barrel,
    "cannon": build_cannon,
    "cannon_barrel": build_cannon_barrel,
    "coilgun": build_coilgun,
    "coilgun_barrel": build_coilgun_barrel,
    "gatling": build_gatling,
    "gatling_barrel": build_gatling_barrel,
    "mining_drill": build_mining_drill,
    "mining_drill_barrel": build_mining_drill_barrel,
    "laser": build_laser,
    "plasma_caster": build_plasma_caster,
    "ion_disruptor": build_ion_disruptor,
    "emp_pulse": build_emp_pulse,
    "tractor_beam": build_tractor_beam,
    "heavy_missile": build_heavy_missile,
    "guided_missile": build_guided_missile,
    "cluster_rocket": build_cluster_rocket,
    "ammo_autoloader": build_ammo_autoloader,
}



def main():
    args = argv_after_ddash()
    which = args[0] if args else "railgun"
    out = args[1] if len(args) > 1 else "/tmp/%s.png" % which
    res = int(args[2]) if len(args) > 2 else 1024

    scene = new_scene()
    m = materials()
    BUILDERS[which](m)
    light_rig(scene)
    configure(scene, res, samples=200)
    if which.endswith("_barrel"):
        # Barrel sprites render at 132 world units against a 66-unit module
        # cell (spawner.rs), so their image spans TWO cells. Framing them at
        # one cell renders a barrel that appears half length in game.
        ortho_camera(scene, CELL * 2.0)
    elif which in WIDE:
        # 2x1 footprint. spawner.rs draws a module at (60 + bounds*66) world
        # units per axis, where bounds is the cell-span DIFFERENCE: a 2x1 is
        # 126 x 60, i.e. 2.1:1 -- not 2:1. The shipped railgun_2x1.png is
        # 576x378 (1.52:1) and is therefore stretched ~38% in game.
        aspect = 126.0 / 60.0
        ortho_camera(scene, CELL * aspect)
        scene.render.resolution_x = res
        scene.render.resolution_y = int(round(res / aspect))
        render_to(scene, out)
        return
    elif which in OVERHANG:
        # Launchers protrude forward past their block, exactly like the
        # existing torpedo_tube art (378x541). The plate stays centred on the
        # cell and the tubes reach into the extra canvas, so the camera is
        # offset rather than the geometry.
        span = CELL * (OVERHANG_H / 378.0)
        cam = ortho_camera(scene, span)
        cam.location = (0.0, span * 0.5 - 0.5, 6.0)
        scene.render.resolution_x = int(res * 378.0 / OVERHANG_H)
        scene.render.resolution_y = res
        render_to(scene, out)
        return
    else:
        ortho_camera(scene, CELL)
    render_to(scene, out)


main()
