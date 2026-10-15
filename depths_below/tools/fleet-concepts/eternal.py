"""Eternal Hegemony: hand-drawn, top-down only.

Lore: the Terran Hegemony's own past, frozen at its peak. It believes it is
two hundred years ago, takes orders from emperors long dead, slips in and out
of time, and can call back past versions of ships it has lost. A boss faction
(danger 6), met as a lone patrol far out.

Grammar: the Terran grammar made monumental and ceremonial.
  - a stepped CROWN BOW: a long central spire flanked by crown points, every
    raked step plated;
  - DOUBLE-TIER SPONSONS: each gun sponson is a ziggurat, a broad gallery with
    a narrower turret tier stacked outboard of it, plated on its forward steps;
  - ENGINE PODS at the stern corners round a recessed centre engine bank;
  - a PROCESSIONAL NAVE down the centreline, lined with shield and core
    "pillars", leading to the throne (the bridge) in the middle of the ship;
  - cathedral symmetry, open courtyards inside the big hulls, Titanium.

Legend: see terran.py and draw.LEGEND.
"""
from draw import Canvas, AIMED, ENGINES, LEGEND
import fleet

FACTION = {
    "slug": "eternal_hegemony",
    "name": "Eternal Hegemony",
    "tagline": "The Hegemony's own past, frozen at its peak. It obeys dead emperors, slips in and out of time, and calls back the ships it has lost.",
    "grammar": [
        "Stepped crown bow: a long central spire between crown points, every raked step plated",
        "Double-tier gun sponsons: a broad gallery with a narrower turret tier stacked outboard",
        "Engine pods at the stern corners round a recessed centre engine bank",
        "A processional nave down the centreline, lined with pillars, ending at the throne-bridge",
        "Cathedral symmetry, open courtyards in the big hulls, Titanium",
    ],
    "accent": "#d4a35a",
}


def finish(c):
    """c.build(), plus two checks the shared checker leaves out: every gun and
    launcher has an open side to fire from, and every engine has clear space
    aft of it."""
    d = c.build()
    hull = {p for p, ch in c.cells.items() if ch != "/"}
    for p, ch in c.cells.items():
        kind = LEGEND.get(ch)
        x, y = p
        if kind in AIMED and kind != "DockingPort":
            if all((x + dx, y + dy) in hull or c.cells.get((x + dx, y + dy)) == "/"
                   for dx, dy in ((0, 1), (0, -1), (1, 0), (-1, 0))):
                d["_checks"].append(f"{kind} at {p} has no open side")
        if kind in ENGINES and any((xx, y) in hull for xx in range(x - 30, x)):
            d["_checks"].append(f"{kind} at {p} is blocked aft")
    return d


def echo_escort():
    """Frigate. The 'old pattern' line ship: the Terran Bulwark two centuries
    ago, heavier and dressed for parade. Crown bow, two ziggurat sponsons a
    side, the forward pair a step lower, a nave running to the bridge."""
    c = Canvas("eh_echo_escort")
    c.rows(-18, 9, "\n".join([
        "           #n#/",
        "          ##.##/      #n#/",
        "         g.a.a.#/    ##.##/",
        " #####   #O.d.Q.#   g.a.a.#/",
        "E.f.c.###.Q...Q..#/ #m.d.O.#",
        "E.R.b.d.KK.KK.m..." + "##" + "..GG.a.T" + "##/",
        " ##.cMf.Q.O.Q.y.d.M...GG.a.A.###",
        "   E......d.........d.M.....#",
        "   E.LL.s.c.s.m.s..BB.H.s.T.#zn##/",
        "   E" + "." * 15 + "BB" + "." * 13 + "#",
    ]))
    return finish(c.mirror())


def herald():
    """Destroyer. Three ziggurat sponsons a side, each smaller and lower than
    the last, so the hull steps down from a broad stern to a crown bow; a nave
    of shield pillars runs from the engine bank to the throne-bridge. The
    Herald goes ahead of the old fleet and announces it."""
    c = Canvas("eh_herald")
    y3 = list("   E.LL.M.Q.SSS.O.y.Q.m.M.b.c.....a.T.s.M.###")
    y1 = list("." * 50)
    y1[:4] = list("   E")
    y1[5] = "R"
    for i in (7, 10, 13, 16, 19, 22, 31, 34, 37, 40):
        y1[i] = "s"                         # the pillars of the nave
    y1[25] = y1[26] = "B"
    y1[28] = "H"
    y1[42:50] = list("zn#####/")
    c.rows(-26, 10, "\n".join([
        "          #n#n#/",
        "         #a.a.a#/       #n#/",
        "        g.O...Q.#/     ##.##/",
        " #####  #.KK.d.KK#    g.a.a.#/      #nn#/",
        "E.f.c.##.m.....y../   #GG.d.Q#/    ga..a#/",
        "E.R.b.v.KK.KK.O.Q." + "#g##" + ".GG.q.a." + "##/ " + "#.O..s.#/",
        " ##" + "." * 15 + "d" + "." * 11 + "d." + "##" + "." * 8 + "#/",
        "".join(y3),
        "   E.f.c..d.SSS.....Q.....b.c.d..........#",
        "".join(y1),
        "   E" + "." * 21 + "BB" + "." * 22 + "n",
    ]))
    return finish(c.mirror())


def throne_of_ages():
    """Boss dreadnought, laid out like a cathedral seen from above: a long nave
    hull with a crown bow, two aisle hulls held off it by three crossbeams,
    and a great transept tower thrown out on each side at the crossing. The
    gaps between are open courtyards, lit by floodlights. The throne (bridge)
    sits where nave and transept cross."""
    c = Canvas("eh_throne_of_ages")
    # Nave, x -34..40, y 0..5 (drawn from -34).
    y1 = list("." * 74)
    y1[0], y1[2] = "E", "R"
    for i in list(range(5, 31, 3)) + list(range(38, 64, 3)):
        y1[i] = "s"                         # the pillars of the nave
    y1[33] = y1[34] = "B"
    y1[36] = "H"
    y1[66], y1[67] = "z", "n"               # guns in the crown's notch
    y1[68:74] = "######"
    nave = [
        "  " + "######" + "#...#" + "#" * 15 + "#.......#" + "#" * 9 + "#...#" + "#" * 14 + "/",
        "N#.LL.bc...KK.Q.KK.m.y..O.q.q....@@.s.W.s.T.V.V..a.a.a.T.a.a.A.s.##/",
        "N" + "." * 19 + "d" + "." * 23 + "d" + "." * 21 + "###",
        "E.R.b.M.f..Q.Q.O.Q.Q.y.m.Q.q.M.....H.s..!!..M....a.c.b.a.s.T.a...#",
        "".join(y1) + "/",
        "E" + "." * 32 + "BB" + "." * 39 + "n",
    ]
    c.rows(-34, 5, "\n".join(nave))
    # Crossbeams, y 6..11: aft beam, transept, fore beam.
    c.rows(-26, 11, """
#O.k#
#R.R#
#c.cp
#Q.Q#
#k.f#
#...#
""")
    c.rows(-6, 11, """
#SSS.GG.#
#SSS.GG.#
l...d...l
#Q.Q.Q.Q#
#.......#
#KK.d.KK#
""")
    c.rows(12, 11, """
#O.k#
#a.a#
p.d.g
#c.R#
#Q.Q#
#...#
""")
    # Aisle hulls, y 12..16, x -28..22, with their own stepped bows.
    aisle = [
        " #g###" + "..Q....." + "#n#n##" + ".Q.........Q." + "#g#" + "..m....O" + "#g#" + "/",
        "E..R.b.a.KK.a.O.a.Q.Q.O.V.V.Q.Q.a..a.KK.m.a.T.s##/",
        "E" + "." * 12 + "d" + "." * 19 + "d" + "." * 15 + "##",
        "E.c..f.M.b.Q.y.Q.m.Q.b.c.R..R.c..a.a.M.a.a.....##/",
        " ##...#" + "##l###p###l####" + "#...d...#" + "##l###l##" + "#...#" + "##/",
    ]
    c.rows(-28, 16, "\n".join(aisle))
    # Sponsons on the aisles' outer faces, and the transept tower between them.
    for a in (-22, 8):
        c.rows(a, 20, """
  #n#/
 ##.##/
g.a.a.#/
#O.d.Q.#
""")
    c.rows(-8, 22, """
    #nn#/
   ga..a#/
  g.s..s.#/
 g.q....q.#/
g.@@...M.y.#/
#.O..m.m..O.#
""")
    return finish(c.mirror())


def reliquary():
    """Experimental carrier. A mandorla, the pointed almond that frames a relic
    in a cathedral window: an almond ring round two open windows, with the keel
    run straight through the middle carrying the throne. Drone bays line both
    sides of each window and face each other like mirrors; that is where the
    echoes of lost ships come through. Engines fan up the stern quarter, every
    forward step is plated."""
    c = Canvas("eh_reliquary_of_echoes")
    y1 = list("." * 56)                     # x -24..31
    y1[0], y1[2] = "E", "R"
    for x in (-19, -16, -13, -10, -7, -4, 3, 6, 9, 12, 15, 18):
        y1[x + 24] = "s"
    y1[23] = y1[24] = "B"
    y1[26], y1[46] = "H", "T"
    y1[51:56] = "#nzn#"
    c.rows(-24, 16, "\n".join([
        "                       #n#/",
        "                      g...#/",
        "                    #g.a.a.##/",
        "                 E#n.Q.O.O.Q.#n#/",
        "              Eg#.R.b.M...M.b.R.#g#/",
        "           Eg#" + "." * 21 + "#g#/",
        "         E....." + "s.a.T.@@.W.@@.T.a.s" + ".....#/",
        "        E..V.V.!!." + "DD#DD#l#DD#DD" + ".!!.V.V..#/",
        "       E..Q.O.QDDl" + " " * 13 + "lDDQ.O.Q..#/",
        "      E..KK.mDD" + " " * 19 + "DDm.KK..#/",
        "     E..M.y.l" + " " * 23 + "l.y.M..#/",
        "    E..Q.QDD" + " " * 25 + "DDQ.Q..#/",
        "   E..b.Ol" + " " * 29 + "lO.b..#/",
        "  E..R.cl" + " " * 31 + "lc.R..#/",
        " E..LL.f." + "#DD#l#DD#DD#l##l##l#DD#DD#l#DD#" + ".a.T..." + "#zn#/",
        "".join(y1) + "/",
        "E" + "." * 22 + "BB" + "." * 31 + "n",
    ]))
    return finish(c.mirror())


def octant(c, top, block):
    """Paint the wedge 0 <= x <= y of a station (rows from y=top down, x from 0),
    reflect it across the diagonal, then turn it four ways: eightfold symmetry."""
    for k, line in enumerate(block.strip("\n").split("\n")):
        y = top - k
        for x, ch in enumerate(line):
            if ch == " ":
                continue
            assert x <= y, f"octant cell {(x, y)} is past the diagonal"
            c.cells[(y, x)] = ch
            if x >= 1:
                c.cells[(x, y)] = ch
    return c.rot4()


def citadel():
    """Station. A grander, double-walled star fort: a sanctum with the throne at
    the centre, an inner wall, an open courtyard crossed by four gatehouses,
    then the curtain wall with a ziggurat gate tower on every face and a
    crowned bastion on every corner. Drawn as one eighth, reflected and turned."""
    c = Canvas("eh_citadel")
    octant(c, 29, """
                     /n/
                    /#.#/
                   /#a.a#/
                   g.O.O.g
                  g.Q.M.Q.
n/               g.b.....
.#/             #.c..FFF
..#g/          n.R.d.FF
.a..#/          #.b..F
.a.a.g           #...
......#n#x#g#x#n#...
.a.s.aa.a.a.a.a.T..
........d.........
.Q.KK.O.m.Q.KK.y.
...##l###p###l##
..#
.s#
..#
.s#
...##l##p##
.Q.q.m.O.Q
....d....
.b.M.R.c
..#l###
.#
.#
""")
    c.rows(-3, 3, """
###d###
#q.s.q#
#.HBB.#
d..BB.d
#..BB.#
#q.s.q#
###d###
""")
    return finish(c)


CONCEPTS = [
    ("echo_escort", echo_escort, "Echo Escort", "Frigate",
     "An 'old pattern' Terran line ship, two centuries out of date and dressed for parade: crown bow, ziggurat sponsons, a nave to the throne."),
    ("herald", herald, "Herald", "Destroyer",
     "Three ziggurat sponsons a side, shrinking toward a crown bow; a nave of shield pillars runs to the throne-bridge. It goes ahead of the old fleet and announces it."),
    ("throne_of_ages", throne_of_ages, "Throne of Ages", "Boss dreadnought",
     "A cathedral seen from above: a crowned nave, two aisle hulls on crossbeams, a transept tower each side, and lit courtyards between. The throne sits at the crossing."),
    ("reliquary_of_echoes", reliquary, "Reliquary of Echoes", "Experimental: echo carrier",
     "A mandorla, the almond frame round a cathedral relic: two open windows lined with facing drone bays like mirrors, the throne carried on the keel between them. The echoes of lost ships come through the windows."),
    ("citadel", citadel, "Citadel", "Station",
     "A double-walled star fort: a throne sanctum, an inner wall, a courtyard crossed by four gatehouses, then a curtain wall with ziggurat gate towers and crowned corner bastions."),
]
