exec(open('/tmp/ships/design.py').read())

def plan_for(cells):
    xs=[p[0] for p in cells]; x0,x1=min(xs),max(xs)
    ys=[p[1] for p in cells]; ytop,ybot=max(ys),min(ys)
    def X(f): return int(round(x0 + (x1-x0)*f))
    return [
        ('StandardEngine', x0, 1, True), ('StandardEngine', x0, 0, True),
        ('StandardEngine', x0, -1, True), ('StandardEngine', x0, 2, True),
        ('StandardEngine', x0, -2, True),
        ('FuelTank', X(.10), 1, False), ('FuelTank', X(.10), -1, False),
        ('StandardReactor', X(.16), 2, False), ('StandardReactor', X(.16), -2, False),
        ('CoolingPump', X(.14), 1, False), ('CoolingPump', X(.14), -1, False),
        ('HeatVent', X(.12), ytop, True), ('HeatVent', X(.12), ybot, True),
        ('OxygenScrubber', X(.26), 1, False), ('OxygenScrubber', X(.26), -1, False),
        ('BasicQuarters', X(.30), 2, False), ('BasicQuarters', X(.30), -2, False),
        ('RepairBay', X(.28), 0, False),
        ('GalleyMess', X(.36), 2, False),
        ('BridgeWing', X(.44), 1, False),
        ('RadarArray', X(.50), 0, False),
        ('Floodlight', X(.62), 0, True),
        ('MemoryCore', X(.20), 0, False), ('MemoryCore', X(.40), -1, False),
        ('MemoryCore', X(.58), 1, False),
        ('SalvageArm', X(.70), 1, True),
        ('BulkCargoHold', X(.44), -3, False),
        ('AirlockChamber', X(.76), -1, True),
        ('Cannon', X(.80), ytop, True), ('Cannon', X(.80), ybot, True),
        ('Gatling', X(.92), ytop, True), ('Gatling', X(.92), ybot, True),
        ('ShieldEmitter', X(.34), ytop-1, False), ('ShieldEmitter', X(.34), ybot+1, False),
    ]

SHIPS = {}
for key in ('pincer', 'trident', 'crab'):
    cells = cells_of(HULLS[key])
    mods = fit_out(cells, plan_for(cells))
    total, reach, stranded = connected(cells, mods)
    cores = [m for m in mods if m['t'] == 'MemoryCore']
    spread = max((abs(a['x']-b['x']) + abs(a['y']-b['y'])) for a in cores for b in cores) if cores else 0
    SHIPS[key] = dict(cells=cells, mods=mods)
    print('%-8s hull %3d  mods %2d  deck %3d/%3d  stranded %d  cores %d spread %d'
          % (key, len(cells), len(mods), reach, total, len(stranded), len(cores), spread))
    if stranded:
        print('        stranded:', stranded[:6])
