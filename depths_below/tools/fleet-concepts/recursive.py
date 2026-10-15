"""Recursive Kingdom: one person, cloned into a civilisation.

Lore: a single founder copied themselves until the copies became a kingdom.
Every hull is crewed by the same person at different ages, forever arguing
with themselves. The original is still alive somewhere, hiding, and disgusted
by what they became. In game they are the faction nearest the start: raid
waves of 3-5 small identical hulls that attack anything nearby, go kamikaze
below 25% hull and pick over the wrecks afterwards.

Grammar, held at every size (all of it reads from above):
  - ONE clone pod, 9x9, drawn once by hand (POD below) and never redrawn.
    Every ship is copies of that pod stamped at offsets, joined by short
    3-wide connectors. The repetition IS the silhouette.
  - The pod is an octagon with a stepped, plated bow; the four faces each
    carry a port at their middle. A face that is joined becomes a doorway,
    a free stern face carries three engines, a free bow face rockets and a
    gatling, a free flank face two gatlings.
  - Inside, every pod is the same flat: a plus of corridors, an engine room
    aft (reactor, battery, fuel, cooling), the clone's quarters and the helm
    midships, a magazine forward, three memory cores (child, adult, elder).
  - Steel. Cluster rockets and gatlings only.

Legend: see draw.py. * cluster rocket, g gatling, X salvage arm, o docking port.
"""
from draw import Canvas

FACTION = {
    "slug": "recursive_kingdom",
    "name": "Recursive Kingdom",
    "tagline": "One person cloned into a whole civilisation, forever arguing with themselves at different ages. The original is alive, hiding, and disgusted.",
    "grammar": [
        "One identical 9x9 clone pod, drawn once, stamped again and again: the repetition is the identity",
        "Pods joined by short 3-wide connectors, open voids between them; bigger ships are lattices, never bigger pods",
        "Each pod an octagon with a stepped, plated bow; free stern faces carry engines, free bows rockets",
        "Same flat in every pod: plus of corridors, engine room aft, quarters and helm midships, magazine forward",
        "Steel, cluster rockets and gatlings, three memory cores per pod (the clone at three ages)",
    ],
    "accent": "#e08a3c",
}

# --------------------------------------------------------------------------- the pod
# Drawn once, bow to the right. Row 0 is the top. The four faces are written
# with their default (free) arming; _stamp() rewrites them per face.
POD = """
  #g#g#/
 #MQ.Qa#/
#bfO..Ta#
ERc..H..*
E.......g
ERc..m..*
#bfO..Ma#
 #MQ.Qa#/
  #g#g#/
"""
_POD = {}
for _r, _line in enumerate(POD.strip("\n").split("\n")):
    for _x, _ch in enumerate(_line):
        if _ch != " ":
            _POD[(_x - 4, 4 - _r)] = _ch   # centred: x -4..4, y -4..4

# Face cells (local, before rotation), in order along the face.
FACES = {
    "top": [(x, 4) for x in range(-2, 3)],
    "bot": [(x, -4) for x in range(-2, 3)],
    "bow": [(4, y) for y in range(2, -3, -1)],
    "aft": [(-4, y) for y in range(2, -3, -1)],
}
PORT = {"top": (0, 4), "bot": (0, -4), "bow": (4, 0), "aft": (-4, 0)}
OUT = {"top": (0, 1), "bot": (0, -1), "bow": (1, 0), "aft": (-1, 0)}
MODES = {
    "top": {"gun": "#g#g#", "dock": "##.##", "plain": "#####", "salvage": "#X#X#", "berth": "##o##"},
    "bow": {"gun": "#*g*#", "dock": "##.##", "plain": "#####", "salvage": "#XgX#", "gat": "##g##",
            "berth": "##o##"},
    "aft": {"engine": "#EEE#", "dock": "##.##", "plain": "#####", "gun": "#g#g#", "salvage": "#X#X#",
            "berth": "##o##"},
}
MODES["bot"] = MODES["top"]


def _xf(p, rot, flip):
    x, y = p
    if flip:
        x = -x
    for _ in range(rot % 4):
        x, y = -y, x
    return x, y


class Brood:
    """An assembly of identical pods. pod() places one, link() joins two
    faces with a straight (or one-elbow) connector, build() stamps it all."""

    def __init__(self, name):
        self.c = Canvas(name, "Steel")
        self.pods = {}
        self.links = []

    def pod(self, name, cx, cy, rot=0, flip=False, **faces):
        self.pods[name] = dict(at=(cx, cy), rot=rot, flip=flip, faces=dict(faces), docked=set())
        return self

    def port(self, name, face):
        p = self.pods[name]
        dx, dy = _xf(PORT[face], p["rot"], p["flip"])
        return p["at"][0] + dx, p["at"][1] + dy

    def out(self, name, face):
        p = self.pods[name]
        return _xf(OUT[face], p["rot"], p["flip"])

    def toward(self, name, d):
        """The local face of a pod that looks in world direction d."""
        p = self.pods[name]
        return next(f for f in OUT if _xf(OUT[f], p["rot"], p["flip"]) == d)

    def link(self, a, fa, b, fb):
        self.pods[a]["docked"].add(fa)
        self.pods[b]["docked"].add(fb)
        self.links.append((a, fa, b, fb))
        return self

    # -------------------------------------------------------------- stamping
    def _corridor(self, p, q):
        """Straight 3-wide corridor between cells p and q (exclusive)."""
        (x0, y0), (x1, y1) = p, q
        if y0 == y1:
            for x in range(min(x0, x1) + 1, max(x0, x1)):
                self.c.cells[(x, y0 + 1)] = "#"
                self.c.cells[(x, y0 - 1)] = "#"
                self.c.cells[(x, y0)] = "."
        else:
            for y in range(min(y0, y1) + 1, max(y0, y1)):
                self.c.cells[(x0 + 1, y)] = "#"
                self.c.cells[(x0 - 1, y)] = "#"
                self.c.cells[(x0, y)] = "."

    def _connect(self, a, fa, b, fb):
        p, q = self.port(a, fa), self.port(b, fb)
        if p[0] == q[0] or p[1] == q[1]:
            self._corridor(p, q)
            return
        # one elbow: leave a along its face normal, turn once to reach b
        ox, oy = self.out(a, fa)
        corner = (q[0], p[1]) if ox else (p[0], q[1])
        for dx in (-1, 0, 1):
            for dy in (-1, 0, 1):
                self.c.cells[(corner[0] + dx, corner[1] + dy)] = "#"
        self.c.cells[corner] = "."
        self._corridor(p, corner)
        self._corridor(corner, q)
        # the elbow's outside corner is cut back so it reads as a bend, not a box
        cx, cy = corner
        ax, ay = p[0] - cx, p[1] - cy
        bx, by = q[0] - cx, q[1] - cy
        sx = -1 if (ax > 0 or bx > 0) else 1
        sy = -1 if (ay > 0 or by > 0) else 1
        self.c.cells.pop((cx + sx, cy + sy), None)

    def _footprint(self, name):
        p = self.pods[name]
        return {(p["at"][0] + dx, p["at"][1] + dy)
                for dx, dy in (_xf(l, p["rot"], p["flip"]) for l, ch in _POD.items() if ch != "/")}

    def _mode(self, name, face, occupied):
        p = self.pods[name]
        if face in p["docked"]:
            return "dock"
        if face in p["faces"]:
            return p["faces"][face]
        # free face: look 4 cells out across its width for anything else
        ox, oy = self.out(name, face)
        px, py = self.port(name, face)
        blocked = any((px + ox * k + oy * s, py + oy * k + ox * s) in occupied
                      for k in range(1, 5) for s in range(-2, 3))
        if blocked:
            return "plain"
        if face == "aft":
            if p["rot"] % 4 == 0 and not p["flip"]:
                # engines only on a true stern: nothing behind on these rows
                behind = any((x < px and abs(y - py) <= 2) for (x, y) in occupied)
                return "gun" if behind else "engine"
            return "gun"
        return "gun"

    def build(self):
        for link in self.links:
            self._connect(*link)
        foot = {n: self._footprint(n) for n in self.pods}
        corridors = set(self.c.cells)
        for name, p in self.pods.items():
            occ = corridors.union(*(f for m, f in foot.items() if m != name))
            local = dict(_POD)
            for face, cells in FACES.items():
                mode = self._mode(name, face, occ - foot[name])
                for cell, ch in zip(cells, MODES[face][mode]):
                    local[cell] = ch
            cx, cy = p["at"]
            for l, ch in local.items():
                dx, dy = _xf(l, p["rot"], p["flip"])
                self.c.cells[(cx + dx, cy + dy)] = ch
        return self.c.build()


S = 11   # lattice pitch: a 9-cell pod plus a 2-cell connector


def lattice(name, spots, links, faces=None, rots=None):
    """Pods on the lattice at (i, j), optionally turned (rots[(i, j)] = 0..3,
    bow east/north/west/south); links are pairs of spots that touch."""
    faces, rots = faces or {}, rots or {}
    b = Brood(name)
    for (i, j) in spots:
        b.pod((i, j), i * S, j * S, rot=rots.get((i, j), 0), **faces.get((i, j), {}))
    for a, c in links:
        d = (c[0] - a[0], c[1] - a[1])
        b.link(a, b.toward(a, d), c, b.toward(c, (-d[0], -d[1])))
    return b


# --------------------------------------------------------------------------- ships
def skiff():
    """Raider. One pod on its own: the clone in a can."""
    return Brood("rk_skiff").pod("p", 0, 0).build()


def twin():
    """Two pods side by side, joined flank to flank."""
    return lattice("rk_twin", [(0, 0), (0, 1)], [((0, 0), (0, 1))]).build()


def brood_raft():
    """Five pods in a plus: four clones round the eldest."""
    spots = [(0, 0), (-1, 0), (1, 0), (0, 1), (0, -1)]
    return lattice("rk_brood_raft", spots, [((0, 0), s) for s in spots[1:]]).build()


def elder_carrier():
    """Carrier. Twelve pods in a lattice round two pod-sized courtyards; every
    face that looks into a courtyard is a docking port, a berth for a Skiff.
    The stern column of five is all engines; the lone bow pod leads."""
    spots = [(-3, j) for j in range(-2, 3)] + [(-2, 2), (-2, 0), (-2, -2), (-1, 1), (-1, 0), (-1, -1), (0, 0)]
    links = [((-3, j), (-3, j + 1)) for j in range(-2, 2)]
    links += [((-3, 2), (-2, 2)), ((-3, -2), (-2, -2)), ((-3, 0), (-2, 0)), ((-2, 0), (-1, 0)),
              ((-1, 0), (0, 0)), ((-1, 0), (-1, 1)), ((-1, -1), (-1, 0))]
    # every face round the two courtyards is a docking port: Skiff berths
    faces = {}
    for s in (1, -1):
        out, back = ("top", "bot") if s > 0 else ("bot", "top")
        faces[(-3, s)] = dict(bow="berth")
        faces[(-2, 0)] = dict(faces.get((-2, 0), {}), **{out: "berth"})
        faces[(-2, 2 * s)] = {back: "berth"}
        faces[(-1, s)] = dict(aft="berth")
    return lattice("rk_elder_carrier", spots, links, faces).build()


def recycler():
    """Scavenger. Five pods in a U round an empty pod-sized mouth; every face
    that looks into the mouth is salvage arms. It parks over a wreck and eats."""
    spots = [(0, 1), (0, 0), (0, -1), (1, 1), (1, -1)]
    links = [((0, 0), (0, 1)), ((0, -1), (0, 0)), ((0, 1), (1, 1)), ((0, -1), (1, -1))]
    faces = {(0, 0): dict(bow="salvage"), (1, 1): dict(bot="salvage"),
             (1, -1): dict(top="salvage")}
    return lattice("rk_recycler", spots, links, faces).build()


def argument():
    """Experimental. Two clusters of three pods face each other down a long
    spine, every rocket pointed inward. The clone, arguing with itself. The
    stern cluster drives; the bow cluster is the same pods turned round."""
    b = Brood("rk_argument")
    gap = 15                         # spine length between the two bows
    for j in (-1, 0, 1):
        b.pod(("L", j), 0, j * S)
        b.pod(("R", j), 8 + gap + 1, j * S, flip=True, aft="gun")
    for side in ("L", "R"):
        b.link((side, -1), "top", (side, 0), "bot")
        b.link((side, 0), "top", (side, 1), "bot")
    b.link(("L", 0), "bow", ("R", 0), "bow")
    return b.build()


def hive():
    """Station. A ring of eight pods round an empty courtyard, with a pod out
    on each point of the compass. Every pod is turned so its bow looks out;
    the faces that look into the courtyard are blank."""
    ring = [(1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1), (0, -1), (1, -1)]
    tips = [(2, 0), (0, 2), (-2, 0), (0, -2)]
    rots = {(1, 0): 0, (0, 1): 1, (-1, 0): 2, (0, -1): 3,
            (1, 1): 0, (-1, 1): 1, (-1, -1): 2, (1, -1): 3}
    rots.update({t: k for k, t in enumerate(tips)})
    links = [(ring[k], ring[(k + 1) % 8]) for k in range(8)]
    links += [(ring[2 * k], tips[k]) for k in range(4)]
    faces = {ring[2 * k]: dict(aft="plain") for k in range(4)}
    return lattice("rk_hive", ring + tips, links, faces, rots).build()


CONCEPTS = [
    ("skiff", skiff, "Skiff", "Raider (waves of 3-5)",
     "One pod on its own: three engines, two rockets, five gatlings and the clone. They come in waves and dive into you below a quarter hull."),
    ("twin", twin, "Twin", "Scout",
     "Two identical pods joined flank to flank. Two helms, one argument."),
    ("brood_raft", brood_raft, "Brood Raft", "Corvette",
     "Five pods in a plus round the eldest. Three sterns drive it, three bows shoot."),
    ("recycler", recycler, "Recycler", "Scavenger",
     "Five pods in a U round an empty pod-sized mouth, every face that looks into it lined with salvage arms. It parks over a wreck and eats."),
    ("argument", argument, "The Argument", "Experimental",
     "Two clusters of the same three pods face each other down a long spine, every rocket aimed inward. The clone, arguing with itself."),
    ("elder_carrier", elder_carrier, "Elder Carrier", "Carrier",
     "Twelve pods in a lattice round two pod-sized courtyards lined with docking ports, where Skiffs berth. A stern wall of fifteen engines; the lone bow pod is the eldest."),
    ("hive", hive, "Hive", "Station",
     "Eight pods in a ring round an empty courtyard and one out on each point of the compass, every bow turned outward."),
]
