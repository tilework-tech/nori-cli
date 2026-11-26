"""Print the emulated screen of an asciinema cast at given timestamps,
and the first time each given needle appears. Usage: screen_at.py cast t1,t2 [needle ...]"""
import json, sys
import pyte
path, times = sys.argv[1], [float(t) for t in sys.argv[2].split(",") if t]
needles = {n: None for n in sys.argv[3:]}
with open(path) as f:
    h = json.loads(f.readline()); scr = pyte.Screen(h["width"], h["height"]); st = pyte.Stream(scr)
    for line in f:
        t, kind, data = json.loads(line)
        if kind == "i":
            print(f"input  {t:8.3f} {data!r}")
        if kind != "o": continue
        while times and t > times[0]:
            print(f"--- screen at {times.pop(0):.3f}"); print("\n".join(l.rstrip() for l in scr.display))
        st.feed(data)
        for n in needles:
            if needles[n] is None and any(n in l for l in scr.display): needles[n] = t
for n, t in needles.items(): print(f"first {n!r}: {t}")
