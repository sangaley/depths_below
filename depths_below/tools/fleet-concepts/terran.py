"""Terran Hegemony: the low-tech line navy.

Grammar, held at every size so a silhouette alone says whose it is:
  - a straight rectangular SPINE, symmetric top to bottom;
  - stepped CASEMATES: rectangular gun decks stacked out from the spine like a
    ziggurat, guns lining every deck edge (broadsides);
  - a flat armoured HAMMER BOW, wider than the spine, chamfered with plate;
  - one dorsal BRIDGE TOWER, set aft of amidships, the only thing that breaks
    the symmetry;
  - a squared-off ENGINE BLOCK aft.
Titanium throughout. Armour only on the bow chamfers and a belt over the engines.

Every gun line has a walkway one row inboard of it, so crew can reach the guns.
"""
from gen import Ship

ENGINEERING = ["LargeReactor", "BatteryBank", "FuelTank", "_", "CoolingPump", "BatteryBank", "HeatVent", "_"]
CREW = ["GalleyMess", "Barracks", "_", "BasicQuarters", "OxygenScrubber", "SurgicalBay", "BasicQuarters", "_", "MedBay"]
STORES = ["BulkCargoHold", "AmmoBay", "_", "LargeCargo", "AmmoBay", "_"]
FORE = ["AmmoBay", "TargetingComputer", "_", "AmmoBay", "RadarArray", "_"]
CARGO = ["BulkCargoHold", "LargeCargo", "SmallCargo"]

FACTION = {
    "slug": "terran_hegemony",
    "name": "Terran Hegemony",
    "tagline": "Low-tech line navy. Humanist, disciplined, built to take a beating and give a broadside back.",
    "grammar": [
        "Straight rectangular spine, symmetric top to bottom",
        "Stepped casemate gun decks stacked out from the spine, guns on every deck edge (broadsides)",
        "Flat armoured hammer bow, wider than the spine",
        "One dorsal bridge tower set aft: the only asymmetry",
        "Squared engine block; Titanium hull; plates only on bow chamfers and the engine block",
    ],
    "accent": "#a6b3cc",
}


def deck(s, x0, x1, y, mirror=True):
    """A walkway along row y through interior cells only."""
    for x in range(min(x0, x1), max(x0, x1) + 1):
        for c in ((x, y), (x, -y)) if mirror else ((x, y),):
            if s.interior(c) and c not in s.occ:
                s.corr.add(c)


def rib(s, x, y0=-99, y1=99, mirror=True):
    """A crossing walkway down column x through interior cells only."""
    for y in range(y0, y1 + 1):
        for c in ((x, y), (x, -y)) if mirror else ((x, y),):
            if c in s.hull and s.interior(c) and c not in s.occ:
                s.corr.add(c)


def picket():
    s = Ship("th_picket")
    s.rect(-7, 5, -2, 2)                     # spine
    s.hammer(6, 8, 3, ch=1)                  # hammer bow
    s.rect(-10, -8, -3, 3)                   # engine block
    s.trap(-4, 0, 3, 4, front=1, back=0)     # bridge tower (dorsal only)
    deck(s, -9, 7, 0, mirror=False)
    s.col("StandardEngine", -10, [-2, 0, 2], "East")
    s.put("StandardReactor", -8, 2, mirror=True)
    s.put("HelmStation", -3, 3)
    s.put("Cannon", -2, 4, "North")
    s.put("Gatling", 8, 2, "East", mirror=True)
    s.put("Cannon", 8, 0, "East")
    s.put("MemoryCore", -6, 1)
    s.put("MemoryCore", -1, -1)
    s.put("MemoryCore", 4, 1)
    s.put("ShieldEmitter", 2, 1, mirror=True)
    s.connect()
    s.fill(lambda x, y: x < -2, ENGINEERING, mirror=True)
    s.fill(lambda x, y: x >= -2, CREW, mirror=True)
    s.armour(bow_from=6, belts=[(-11, -9)])
    return s.build()


def corvette():
    s = Ship("th_corvette")
    s.rect(-11, 9, -3, 3)
    tips = s.casemate(-8, 4, 4, 6)           # casemate deck
    s.hammer(10, 12, 5, ch=2)
    s.rect(-14, -12, -4, 4)                  # engines
    s.trap(-6, -1, 7, 9, front=1, back=0)    # tower
    s.col("StandardEngine", -14, [-3, -1, 1, 3], "East")
    s.put("StandardReactor", -12, 2, mirror=True)
    s.put("HelmStation", -5, 8)
    s.put("HeavyMissile", -5, 9, "North")
    s.put("Cannon", -3, 9, "North")
    s.line("Cannon", [0, 2], 6, "North", mirror=True)
    s.put("Gatling", -7, 6, "North", mirror=True)
    s.put("Gatling", 4, 5, "East", mirror=True)
    s.put("Gatling", 12, 3, "East", mirror=True)
    s.put("Railgun", 11, 1, "North", mirror=True)
    s.put("MemoryCore", -10, 1)
    s.put("MemoryCore", -2, 2)
    s.put("MemoryCore", 7, -2)
    s.put("ShieldEmitter", 5, 2, mirror=True)
    deck(s, -13, 10, 0, mirror=False)
    deck(s, -7, 3, 5)
    rib(s, -13); rib(s, -4); rib(s, 4); rib(s, 10)
    s.connect()
    s.fill(lambda x, y: x < -4, ENGINEERING, mirror=True)
    s.fill(lambda x, y: -4 < x < 4, CREW, mirror=True)
    s.fill(lambda x, y: x > 4, FORE, mirror=True)
    s.armour(bow_from=9, belts=[(-15, -12)], tips=tips)
    return s.build()


def frigate():
    s = Ship("th_frigate")
    s.rect(-15, 13, -4, 4)
    tips = s.casemate(-11, 8, 5, 7)          # casemate tier 1
    tips |= s.casemate(-8, 4, 8, 10)         # casemate tier 2
    s.hammer(14, 17, 7, ch=3)
    s.rect(-18, -16, -6, 6)                  # engines
    s.trap(-7, -1, 11, 13, front=1, back=0)  # tower
    s.col("StandardEngine", -18, [-5, -3, -1, 1, 3, 5], "East")
    s.put("StandardReactor", -16, 1, mirror=True)
    s.put("StandardReactor", -16, 3, mirror=True)
    s.put("BridgeWing", -6, 13, "East")
    s.put("HeavyMissile", -7, 13, "North")
    s.put("Cannon", -4, 13, "North")
    s.line("Cannon", [-10, 6], 7, "North", mirror=True)
    s.put("Gatling", 8, 6, "East", mirror=True)
    s.line("Cannon", [1, 3], 10, "North", mirror=True)
    s.put("Gatling", 4, 9, "East", mirror=True)
    s.line("Cannon", [-7, -4, -1], -10, "South")
    s.put("Gatling", 17, 4, "East", mirror=True)
    s.put("Railgun", 15, 2, "North", mirror=True)
    s.put("Cannon", 17, 0, "East")
    s.put("MemoryCore", -14, 1)
    s.put("MemoryCore", -1, 3)
    s.put("MemoryCore", 10, -2)
    s.put("ShieldEmitter", 11, 3, mirror=True)
    s.put("ShieldEmitter", -3, 6, mirror=True)
    s.door(-5, 0)
    s.door(5, 0)
    deck(s, -17, 16, 0, mirror=False)
    deck(s, -10, 7, 6)
    deck(s, -7, 3, 9)
    for x in (-17, -12, 11, 14):
        rib(s, x)
    s.connect()
    s.fill(lambda x, y: x < -12, ENGINEERING, mirror=True)
    s.fill(lambda x, y: -12 < x < -5, STORES, mirror=True)
    s.fill(lambda x, y: -5 < x < 11, CREW, mirror=True)
    s.fill(lambda x, y: x > 11, FORE, mirror=True)
    s.armour(bow_from=12, belts=[(-19, -16)], tips=tips)
    return s.build()


def destroyer():
    s = Ship("th_destroyer")
    s.rect(-20, 18, -4, 4)
    tips = s.casemate(-15, 11, 5, 7)
    tips |= s.casemate(-12, 7, 8, 10)
    s.hammer(19, 22, 8, ch=3)
    s.rect(-23, -21, -7, 7)
    s.trap(-10, -3, 11, 13, front=1, back=0)
    s.col("StandardEngine", -23, [-6, -4, -2, 0, 2, 4, 6], "East")
    for y in (1, 3):
        s.put("StandardReactor", -21, y, mirror=True)
    s.put("BridgeWing", -9, 13, "East")
    s.put("HeavyMissile", -10, 13, "North")
    s.put("Cannon", -6, 13, "North")
    s.line("HeavyMissile", [0, 2, 4], 10, "North", mirror=True)
    s.put("Cannon", -2, 10, "North", mirror=True)
    s.line("Cannon", [-10, -7, -4], -10, "South")
    s.line("Cannon", [-14, 9], 7, "North", mirror=True)
    s.put("Gatling", 11, 6, "East", mirror=True)
    s.put("Gatling", 7, 9, "East", mirror=True)
    s.put("Gatling", 22, 5, "East", mirror=True)
    s.put("Railgun", 20, 3, "North", mirror=True)
    s.put("Railgun", 20, 1, "North", mirror=True)
    s.put("MemoryCore", -19, 1)
    s.put("MemoryCore", -3, 3)
    s.put("MemoryCore", 12, -2)
    s.put("ShieldEmitter", 15, 3, mirror=True)
    s.put("ShieldEmitter", -6, 7, mirror=True)
    s.put("PointDefenseDrone", 9, 5, "North", mirror=True)
    s.door(-8, 0)
    s.door(6, 0)
    deck(s, -22, 21, 0, mirror=False)
    deck(s, -14, 10, 6)
    deck(s, -11, 6, 9)
    for x in (-22, -16, -1, 13, 19):
        rib(s, x)
    s.connect()
    s.fill(lambda x, y: x < -16, ENGINEERING, mirror=True)
    s.fill(lambda x, y: -16 < x < -8, STORES, mirror=True)
    s.fill(lambda x, y: -8 < x < 6, CREW, mirror=True)
    s.fill(lambda x, y: 6 < x < 13, STORES, mirror=True)
    s.fill(lambda x, y: x > 13, FORE, mirror=True)
    s.armour(bow_from=17, belts=[(-24, -21)], tips=tips)
    return s.build()


def battleship():
    """The frame: a spine and two broadside gun hulls held apart by girders,
    open space between them (Cosmoteer's big-ship rule, done Terran-style)."""
    s = Ship("th_battleship")
    s.rect(-25, 24, -4, 4)                           # spine
    s.trap(-20, 18, 9, 15, 1, 1, mirror=True)        # broadside gun hulls, stepped outward
    s.rect(-16, -12, 5, 8, mirror=True)              # aft girder
    s.rect(4, 8, 5, 8, mirror=True)                  # fore girder
    s.hammer(25, 28, 10, ch=4)                       # hammer bow
    s.rect(-23, -20, 9, 12, mirror=True)             # gun hull engine pods
    s.rect(-29, -26, -7, 7)                          # engine block
    s.trap(-9, -1, 16, 18, front=1, back=0)          # bridge tower, dorsal gun hull
    for y in (7, 5, 3, 1, -1, -3, -5):
        s.put("LargeEngine", -29, y, "East")
    s.col("StandardEngine", -23, [10, 11], "East", mirror=True)
    s.put("LargeReactor", -27, 2, "North", mirror=True)
    s.put("FusionReactor", -14, 1, "North", mirror=True)
    s.put("BridgeWing", -8, 18, "East")
    s.put("HeavyMissile", -9, 18, "North")
    s.put("HeavyMissile", -4, 18, "North")
    s.put("Cannon", -6, 18, "North")
    s.line("Railgun", [-13, 1, 5, 9], 15, "North", mirror=True)
    s.line("Railgun", [-11, -7, -3], -15, "North")
    for k in range(1, 7):                            # a gun on every outward step
        s.put("Cannon", -20 + k, 9 + k, "North", mirror=True)
        s.put("Cannon", 18 - k, 9 + k, "North", mirror=True)
    s.put("Gatling", 28, 6, "East", mirror=True)
    s.put("Railgun", 26, 1, "North", mirror=True)
    s.put("Railgun", 26, 4, "North", mirror=True)
    s.put("Cannon", 28, 0, "East")
    s.line("Gatling", [-6, 0, 12], 9, "South", mirror=True)
    s.put("MemoryCore", -24, 1)
    s.put("MemoryCore", 0, 2)
    s.put("MemoryCore", 20, -2)
    s.put("MemoryCore", -4, 12, mirror=True)
    s.put("ShieldEmitter", 10, 12, mirror=True)
    s.put("ShieldEmitter", 12, 2, mirror=True)
    s.put("ShieldEmitter", -12, 12, mirror=True)
    s.put("PointDefenseDrone", -14, 7, "North", mirror=True)
    s.put("PointDefenseDrone", 6, 7, "North", mirror=True)
    s.door(-11, 0)
    s.door(9, 0)
    deck(s, -28, 27, 0, mirror=False)
    deck(s, -22, 17, 12)
    deck(s, -19, 17, 10)
    for x in (-28, -22, -18, -14, -4, 6, 14, 20, 25):
        rib(s, x)
    s.connect()
    s.fill(lambda x, y: x < -11 and abs(y) < 5, ENGINEERING, mirror=True)
    s.fill(lambda x, y: -11 < x < 9 and abs(y) < 5, CREW, mirror=True)
    s.fill(lambda x, y: x > 9 and abs(y) < 5, FORE, mirror=True)
    s.fill(lambda x, y: 5 <= abs(y) < 9, STORES, mirror=True)
    s.fill(lambda x, y: abs(y) >= 9 and x < -6, ENGINEERING + STORES, mirror=True)
    s.fill(lambda x, y: abs(y) >= 9 and x >= -6, CREW + ["AmmoBay"], mirror=True)
    s.armour(bow_from=17, belts=[(-30, -26)])
    return s.build()


def hauler():
    """Convoy Hauler: an armoured cab towing a spine of container pods, gaps
    between the pods so each container reads as its own block."""
    s = Ship("th_convoy_hauler")
    s.hammer(8, 14, 4, ch=2)                    # armoured cab
    s.rect(-20, 7, -2, 2)                       # spine
    for x0 in (-18, -12, -6, 0):
        s.rect(x0, x0 + 4, 3, 7, mirror=True)   # container pods
    s.rect(-23, -21, -3, 3)                     # tug engines
    s.col("StandardEngine", -23, [-2, 0, 2], "East")
    s.put("StandardReactor", -21, 1, mirror=True)
    s.put("BridgeWing", 10, 3, "East")
    s.put("Gatling", 13, 3, "East", mirror=True)
    s.put("Gatling", 11, 4, "North", mirror=True)
    s.put("PointDefenseDrone", 8, 4, "North", mirror=True)
    s.put("MemoryCore", -19, 1)
    s.put("MemoryCore", 12, -1)
    s.put("MemoryCore", -6, 1)
    s.put("ShieldEmitter", 9, 1)
    for x0 in (-18, -12, -6, 0):
        s.put("BulkCargoHold", x0 + 1, 6, "North", mirror=True)
        s.put("SmallCargo", x0 + 3, 5, mirror=True)
        s.put("SmallCargo", x0 + 3, 6, mirror=True)
    deck(s, -22, 13, 0, mirror=False)
    s.connect()
    s.fill(lambda x, y: x >= 8, ["BasicQuarters", "OxygenScrubber", "FuelTank", "BasicQuarters"], mirror=True)
    s.fill(lambda x, y: x < 8, CARGO, mirror=True)
    s.armour(bow_from=12, belts=[(-24, -21)])
    return s.build()


def gun_platform():
    """Defense platform: a cross with a turret at each arm, radial, no engines."""
    s = Ship("th_gun_platform")
    s.rect(-2, 2, -6, 6)
    s.rect(-6, 6, -2, 2)
    s.rect(-4, 4, -4, 4)
    s.put("Cannon", 6, 0, "East"); s.put("Cannon", -6, 0, "West")
    s.put("Cannon", 0, 6, "North"); s.put("Cannon", 0, -6, "South")
    s.put("Railgun", 5, 2, "East"); s.put("Railgun", -5, -2, "West")
    s.put("Railgun", -2, 5, "North"); s.put("Railgun", 2, -5, "South")
    s.put("Gatling", 3, 3, "East"); s.put("Gatling", -3, -3, "West")
    s.put("Gatling", -3, 3, "North"); s.put("Gatling", 3, -3, "South")
    s.put("StandardReactor", 1, 1); s.put("StandardReactor", -1, -1)
    s.put("MemoryCore", -3, 2); s.put("MemoryCore", 3, -2); s.put("MemoryCore", 2, 3)
    s.put("ShieldEmitter", -1, 1); s.put("ShieldEmitter", 1, -1)
    deck(s, -5, 5, 0, mirror=False)
    rib(s, 0, mirror=False)
    s.connect()
    s.fill(lambda x, y: True, ["BasicQuarters", "AmmoBay", "BatteryBank", "OxygenScrubber"])
    s.armour(tips=set(c for c, _ in s.corners()))
    return s.build()


def bastion():
    """Battlestation, a star fort: a square curtain wall around a docking basin,
    arrowhead bastions at the four corners covering each other's walls."""
    s = Ship("th_bastion")
    R, r = 14, 6
    s.rect(-R, R, -R, R)
    s.cut(-r, r, -r, r)
    centres = []
    for sx in (-1, 1):
        for sy in (-1, 1):
            cx, cy = sx * (R + 2), sy * (R + 2)
            centres.append((cx, cy))
            for dx in range(-6, 7):
                for dy in range(-6, 7):
                    if abs(dx) + abs(dy) <= 6:
                        s.hull.add((cx + dx, cy + dy))
    # guns on every outward-facing bastion cell
    for (x, y) in sorted(s.hull):
        if max(abs(x), abs(y)) <= R or s.interior((x, y)):
            continue
        out = [d for d in ((0, 1), (0, -1), (1, 0), (-1, 0)) if (x + d[0], y + d[1]) not in s.hull]
        if len(out) != 1:
            continue
        rot = {(0, 1): "North", (0, -1): "South", (1, 0): "East", (-1, 0): "West"}[out[0]]
        if (x + y) % 3 == 0:
            s._place("Cannon", (x, y), rot, strict=False)
        elif (x - y) % 4 == 0:
            s._place("Gatling", (x, y), rot, strict=False)
    # basin-facing guard guns and dock lights
    for i in (-4, 0, 4):
        s.put("Gatling", i, r + 1, "South"); s.put("Gatling", i, -r - 1, "North")
        s.put("Gatling", r + 1, i, "West"); s.put("Gatling", -r - 1, i, "East")
    # walkways: a ring mid-wall, and a ring inside each bastion
    mid = (R + r + 1) // 2
    for i in range(-mid, mid + 1):
        for c in ((i, mid), (i, -mid), (mid, i), (-mid, i)):
            if s.interior(c) and c not in s.occ:
                s.corr.add(c)
    for cx, cy in centres:
        for dx in range(-5, 6):
            for dy in range(-5, 6):
                if abs(dx) + abs(dy) == 4:
                    c = (cx + dx, cy + dy)
                    if s.interior(c) and c not in s.occ:
                        s.corr.add(c)
    s.put("FusionReactor", -13, 1); s.put("FusionReactor", 11, -3)
    s.put("BridgeWing", 1, 13, "East")
    s.put("ShieldEmitter", -mid + 1, mid - 1); s.put("ShieldEmitter", mid - 1, -mid + 1)
    s.put("ShieldEmitter", mid - 1, mid - 1); s.put("ShieldEmitter", -mid + 1, -mid + 1)
    for c in ((-12, -3), (12, 3), (-3, 12), (3, -12)):
        s.put("MemoryCore", *c)
    s.connect()
    s.fill(lambda x, y: True, ["Barracks", "AmmoBay", "BulkCargoHold", "BatteryBank", "MessHall",
                               "OxygenScrubber", "MedBay", "DroneBay", "RepairBay", "LargeCargo"])
    s.armour(tips=set(c for c, _ in s.corners() if max(abs(c[0]), abs(c[1])) > R + 2))
    return s.build()


CONCEPTS = [
    ("picket", picket, "Picket", "Scout",
     "Fast patrol and sensor picket. The first Hegemony hull a new captain meets."),
    ("corvette", corvette, "Corvette", "Corvette",
     "One casemate deck with three cannons a side. About Pincer-sized: your peer."),
    ("frigate", frigate, "Line Frigate", "Frigate",
     "Two casemate tiers, a railgun hammer bow, and a bridge tower with a missile on top. The navy's workhorse."),
    ("destroyer", destroyer, "Destroyer", "Destroyer",
     "Missile batteries on the upper casemate, a gun line on each deck, point-defence drones forward."),
    ("battleship", battleship, "Battleship", "Battleship",
     "A spine and two gun hulls held apart by girders, with open space between them. Broadsides both inward and outward."),
    ("hauler", hauler, "Convoy Hauler", "Trade",
     "An armoured cab towing eight container pods. Military logistics: escorted and lightly armed."),
    ("gun_platform", gun_platform, "Gun Platform", "Defense platform",
     "A static, cross-shaped battery that rings a Hegemony station or protected planet."),
    ("bastion", bastion, "Bastion", "Battlestation",
     "A star fort: a curtain wall around a docking basin, with arrowhead bastions at the corners. It sits by the Hegemony capital."),
]
