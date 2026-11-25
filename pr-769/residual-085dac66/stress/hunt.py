"""Repeat failed-session recovery in a draining tmux and record whether a disconnect notice appeared."""
import json
import os
import signal
import sys
import tempfile
import time

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "harness/perf"))
from kiln import KilnHost, Pane  # noqa: E402
from replay769 import history_text  # noqa: E402

binary, tag, n = sys.argv[1], sys.argv[2], int(sys.argv[3])
sections = int(sys.argv[4]) if len(sys.argv) > 4 else 64
for attempt in range(1, n + 1):
    native = attempt % 2 == 0
    with KilnHost(binary, sections, native_load=native) as h, tempfile.TemporaryDirectory() as d:
        pid = next(e["pid"] for e in h.ledger() if e["event"] == "seeded")
        os.kill(pid, signal.SIGTERM)
        while "failed" not in h.run("ls"):
            time.sleep(0.02)
        p = Pane(d, h, [h.nori, "attach"], 100, 30)
        try:
            p.start()
            p.wait(lambda s: "1 failed" in s and "enter open" in s)
            p.key("C-r")
            p.wait(lambda s: "1 ready" in s and "enter open" in s)
            t = time.time_ns() // 1000
            p.key("Enter")
            p.wait(lambda s: "final.txt" in history_text(s), timeout=20)
            p.settle(0.3)
            hit = b"Connection lost" in p.raw.read_bytes()
            print(json.dumps({"tag": tag, "attempt": attempt, "native": native,
                              "enter_us": t, "hit": hit}), flush=True)
        except Exception as e:  # record and keep going
            print(json.dumps({"tag": tag, "attempt": attempt, "error": str(e)[:300]}), flush=True)
        finally:
            p.close()
