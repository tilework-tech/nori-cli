import os, signal, sys, tempfile, time
sys.path.insert(0, "/home/sprite/org/workspace/cli/.worktrees/accept769/harness/perf")
from kiln import KilnHost, Pane
from replay769 import history_text
out = sys.argv[2]
for attempt in range(1, 25):
    with KilnHost(sys.argv[1], 64, native_load=attempt % 2 == 0) as h, tempfile.TemporaryDirectory() as d:
        pid = next(e["pid"] for e in h.ledger() if e["event"] == "seeded")
        os.kill(pid, signal.SIGTERM)
        while "failed" not in h.run("ls"): time.sleep(0.02)
        p = Pane(d, h, [h.nori, "attach"], 100, 30)
        try:
            p.start(); p.wait(lambda s: "1 failed" in s and "enter open" in s)
            p.key("C-r"); p.wait(lambda s: "1 ready" in s and "enter open" in s)
            p.key("Enter"); p.wait(lambda s: "final.txt" in history_text(s)); p.settle(0.3)
            raw = p.raw.read_bytes()
            if b"Connection lost" not in raw:
                print("attempt", attempt, "no disconnect"); continue
            for _ in range(200):
                if "Connection lost" in history_text(p.screen()): break
                before = history_text(p.screen()); p.key("PageUp")
                p.wait(lambda s: history_text(s) != before, timeout=2); p.settle(0.1)
            if "Connection lost" not in history_text(p.screen()):
                print("attempt", attempt, "disconnect only transient"); continue
            p.key("PageDown"); p.settle(0.1); p.key("PageUp"); s = p.settle(0.2)
            open(out + ".ansi", "w").write(p.ansi().rstrip("\n"))
            open(out + ".txt", "w").write(s)
            print("captured on attempt", attempt, "native" if attempt % 2 == 0 else "transcript"); break
        finally:
            p.close()
