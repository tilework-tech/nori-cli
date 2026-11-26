"""Live multi-line stream on a joined attach: when does each line appear, does the
partial tail wait for its end, and after Ctrl+L is every live line printed once?"""
import json, re, sys, tempfile, time
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parent))
from kiln import KilnHost, NS
from joined_open import DeepPane
binary, label = sys.argv[1], sys.argv[2]
out = {"build": label, "runs": []}
for run in range(3):
    with KilnHost(binary, 20) as host, tempfile.TemporaryDirectory(prefix=".lv-") as d:
        pane = DeepPane(d, dict(host.env), host.cwd, [host.nori, "attach", host.session_id[:8]], 100, 30)
        try:
            pane.start(); pane.wait(lambda s: "KilnLatestEnd" in s, 60); pane.settle(0.4)
            t0 = NS(); host.live(1, chunks=9, interval_ms=120)
            seen, partial_early = {}, False
            end = time.monotonic() + 8
            while time.monotonic() < end:
                s = pane.screen(); now = (NS() - t0) / 1e6
                for m in re.finditer(r"LiveLine(\d\d)", s): seen.setdefault(int(m.group(1)), round(now, 1))
                if "PartialTail" in s and "KilnLive0001" not in s: partial_early = True
                if "KilnLive0001" in s: seen.setdefault("end", round(now, 1)); break
                time.sleep(0.005)
            sent = {e["c"]: (e["monotonic_ns"] - t0) / 1e6 for e in host.ledger() if e["event"] == "live_line"}
            lag = {c: round(seen[c] - sent[c], 1) for c in sent if c in seen}
            pane.tmux("send-keys", "-t", "k", "C-l"); pane.settle(0.8)
            hist = pane.history()
            counts = {c: hist.count(f"LiveLine{c:02d}") for c in range(1, 9)}
            out["runs"].append({"lag_ms": lag, "partial_shown_before_end": partial_early,
                                "end_ms": seen.get("end"), "ctrl_l_counts": counts,
                                "tail_once": hist.count("PartialTail0001") == 1})
        finally:
            pane.close()
print(json.dumps(out))
