#!/usr/bin/env python3
"""Where do duplicated kiln marks come from after an alternate-screen attach switches inline?

Opens a kiln host of N sections with `nori attach`, waits for the newest reply, then
either keeps it on the alternate screen (`--mode alt`, audit the visible screen only)
or presses Ctrl+L (`--mode ctrl-l`) and audits the scrollback; prints each duplicated
mark with the rows around it."""
import argparse, json, re, sys, tempfile, time
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parent))
from kiln import KilnHost  # noqa: E402
from joined_open import DeepPane, MARK  # noqa: E402

p = argparse.ArgumentParser()
p.add_argument("--nori", required=True); p.add_argument("--sections", type=int, default=150)
p.add_argument("--out", type=Path, required=True); p.add_argument("--wait-before-switch", type=float, default=0.5)
a = p.parse_args()
with KilnHost(str(Path(a.nori).resolve()), a.sections) as host, tempfile.TemporaryDirectory(prefix=".dp-") as d:
    pane = DeepPane(d, dict(host.env), host.cwd, [host.nori, "attach", host.session_id[:8]], 100, 30)
    try:
        pane.start()
        pane.wait(lambda s: "KilnLatestEnd" in s, 60)
        pane.settle(a.wait_before_switch)
        pane.tmux("send-keys", "-t", "k", "C-l")
        time.sleep(3.0)
        hist = pane.history()
    finally:
        pane.close()
a.out.write_text(hist)
marks = [int(m.group(1)) for m in MARK.finditer(hist)]
dups = sorted({m for m in marks if marks.count(m) > 1})
lines = hist.splitlines()
print(json.dumps({"sections": a.sections, "marks": len(marks), "dups": dups}))
for m in dups[:3]:
    idx = [i for i, l in enumerate(lines) if f"KilnMark{m:04d}" in l]
    for i in idx:
        print(f"--- KilnMark{m:04d} at line {i}")
        print("\n".join(lines[max(0, i - 2):i + 3]))
