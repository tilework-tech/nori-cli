import sys, tempfile, time
sys.path.insert(0, "/home/sprite/org/workspace/cli/.worktrees/accept769/harness/perf")
from kiln import KilnHost, Pane
from replay769 import history_text
for label, b in [("base", sys.argv[1]), ("fix", sys.argv[2])]:
    with KilnHost(b, 8) as h, tempfile.TemporaryDirectory() as d:
        p = Pane(d, h, [h.nori, "attach"])
        p.start(); p.wait(lambda s: "1 ready" in s and "enter open" in s)
        p.key("Enter"); p.wait(lambda s: "final.txt" in s); p.settle(0.2)
        n = 0
        while True:
            before = history_text(p.screen()); p.key("PageUp"); n += 1
            try: p.wait(lambda s: history_text(s) != before, timeout=0.6)
            except TimeoutError: break
        top = history_text(p.settle(0.2))
        for _ in range(5): p.key("PageUp")
        p.settle(0.2)
        moves = 0
        for i in range(8):
            before = history_text(p.screen()); p.key("PageDown")
            try:
                p.wait(lambda s: history_text(s) != before, timeout=0.6); moves = i + 1; break
            except TimeoutError: pass
        print(label, "pageups to top", n, "top has opening", "KilnOpening" in top, "PageDown presses before view moved after 5 extra PageUps:", moves)
        p.close()
