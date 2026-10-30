"""Trains the enemy's mind (src/ai_ship/awareness.rs, `Mind`) on real fights.

Hill-climbs the dials: each dial is tried up and down against the same set of
fights, muted and offscreen, and kept where the enemy does better. Better means
more of the player's ship gone for less of its own:

    fitness = mean(player blocks lost - enemy blocks lost), each 0..1

    python3 tools/combat-lab/train.py <out_dir> <binary> [secs] [workers]
    python3 tools/combat-lab/train.py <out_dir> <binary> --compare <old_binary> [secs] [workers]

Run from depths_below/, with a copy of the built binary so a rebuild can't
change it mid-run. --compare runs every faction on both binaries instead.
"""
import json, os, re, subprocess, sys, time
from concurrent.futures import ThreadPoolExecutor

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
ALL = ["EternalHegemony", "TerranHegemony", "GildedThrone", "BrokenChoir", "CorpseStars",
       "SynthesisCollective", "Shepherd", "RecursiveKingdom"]
# The factions that think: the Choir fight by reflex and the dials barely touch them.
TRAIN_ON = ["TerranHegemony", "GildedThrone", "EternalHegemony", "SynthesisCollective", "CorpseStars", "RecursiveKingdom"]
START = dict(jink=0.6, focus=2.0, withdraw=1.0, kite_margin=600.0, close_to=0.55, recover_shield=0.15, salvo_beat=8.0, flank=0.6)
STEPS = dict(jink=[0.3, 1.0], focus=[1.3, 3.5], withdraw=[0.6, 1.5], kite_margin=[200.0, 1200.0],
             close_to=[0.4, 0.75], recover_shield=[0.05, 0.35], salvo_beat=[0.0, 14.0], flank=[0.3, 1.0])
# An improvement has to beat this to count: fights have noise of their own.
MARGIN = 0.015
LINE = re.compile(r"COMBAT t=(\d+) player hull=[\d.]+ blocks=(\d+)/(\d+) .*?\| (.*)")
ENEMY = re.compile(r"(\w+)/(?:\w+|-) hull=[\d.]+ blocks=(\d+)/(\d+) d=\d+ (\w+)")


def fight(binary, faction, secs, weights, out, tag):
    log = os.path.join(out, f"{tag}_{faction}.log")
    env = dict(os.environ, DEPTHS_MUTE="1", DEPTHS_SKIP_MENU="1", DEPTHS_OFFSCREEN="1", DEPTHS_MOVETEST="1",
               DEPTHS_MOVETEST_ENEMY_SHOOTS="1", DEPTHS_MOVETEST_FLEET=f"{faction}:Patrol",
               DEPTHS_MOVETEST_FLEET_AT="0,6000", DEPTHS_COMBAT_REPORT="1", DEPTHS_DESIGN="designs/starter.json",
               DEPTHS_KEYS=f"3:Backslash,4:Space~{secs}", NO_COLOR="1",
               DEPTHS_AI_WEIGHTS=",".join(f"{k}={v}" for k, v in weights.items()))
    with open(log, "w") as f:
        p = subprocess.Popen(["caffeinate", "-di", binary], cwd=ROOT, env=env, stdout=f, stderr=subprocess.STDOUT)
        time.sleep(secs + 6)
        p.terminate()
        try:
            p.wait(10)
        except subprocess.TimeoutExpired:
            p.kill()
    text = re.sub(r"\x1b\[[0-9;]*m", "", open(log, errors="replace").read())
    rows = LINE.findall(text)
    if not rows:
        return dict(faction=faction, ok=False, fit=0.0, player=0.0, enemy=0.0, killed=0, ships=0, panics=text.count("panicked"))
    _, alive, start, enemies = rows[-1]
    player = 1 - int(alive) / max(int(start), 1)
    best = {}
    # Ships leave the report if they go far off: use every line's worst for each slot.
    found = ENEMY.findall(enemies)
    gone = total = killed = 0
    for _, blocks, first, state in found:
        total += int(first)
        if state == "DESTROYED":
            gone += int(first)
            killed += 1
        else:
            gone += int(first) - int(blocks)
    enemy = gone / max(total, 1)
    # Trace-free runs keep the log small; don't leave it behind.
    os.remove(log)
    return dict(faction=faction, ok=True, fit=player - enemy, player=player, enemy=enemy, killed=killed, ships=len(found), panics=text.count("panicked"))


def evaluate(pool, binary, weights, secs, out, tag, factions=TRAIN_ON):
    jobs = [pool.submit(fight, binary, f, secs, weights, out, tag) for f in factions]
    res = [j.result() for j in jobs]
    good = [r for r in res if r["ok"]]
    return sum(r["fit"] for r in good) / max(len(good), 1), res


def main():
    out, binary = sys.argv[1], os.path.abspath(sys.argv[2])
    args = sys.argv[3:]
    compare = None
    if args and args[0] == "--compare":
        compare = os.path.abspath(args[1])
        args = args[2:]
    secs = int(args[0]) if args else 100
    workers = int(args[1]) if len(args) > 1 else 3
    os.makedirs(out, exist_ok=True)
    report = open(os.path.join(out, "training.md"), "a")

    def say(line):
        print(line, flush=True)
        report.write(line + "\n")
        report.flush()

    with ThreadPoolExecutor(workers) as pool:
        if compare:
            say("| faction | old: your loss | old: their loss | old: killed | new: your loss | new: their loss | new: killed |\n|---|---|---|---|---|---|---|")
            _, old = evaluate(pool, compare, {}, secs, out, "old", ALL)
            _, new = evaluate(pool, binary, {}, secs, out, "new", ALL)
            for o, n in zip(old, new):
                say(f"| {o['faction']} | {o['player']:.0%} | {o['enemy']:.0%} | {o['killed']}/{o['ships']} | {n['player']:.0%} | {n['enemy']:.0%} | {n['killed']}/{n['ships']} |")
            return
        best = dict(START)
        score, res = evaluate(pool, binary, best, secs, out, "base")
        again, _ = evaluate(pool, binary, best, secs, out, "base2")
        say(f"start {best}\nfitness {score:+.3f}, repeat {again:+.3f} (the noise)\n" + "\n".join(f"  {r['faction']}: you -{r['player']:.0%}, them -{r['enemy']:.0%}, killed {r['killed']}/{r['ships']}" for r in res))
        score = (score + again) / 2
        for dial, values in STEPS.items():
            for v in values:
                trial = dict(best, **{dial: v})
                s, _ = evaluate(pool, binary, trial, secs, out, "trial")
                keep = s > score + MARGIN
                say(f"{dial}={v}: {s:+.3f} vs {score:+.3f} {'KEPT' if keep else ''}")
                if keep:
                    best, score = trial, s
        final, res = evaluate(pool, binary, best, secs, out, "final")
        say(f"trained {best}\nfitness {final:+.3f}\n" + "\n".join(f"  {r['faction']}: you -{r['player']:.0%}, them -{r['enemy']:.0%}, killed {r['killed']}/{r['ships']}, panics {r['panics']}" for r in res))
        json.dump(best, open(os.path.join(out, "trained.json"), "w"), indent=1)


if __name__ == "__main__":
    main()
