"""E1: newline hint follows the active keyboard mode (synthetic)."""
import sys, time, json
from pathlib import Path
from drv import *
nori = Path(sys.argv[1]).resolve(); out = Path(sys.argv[2]).resolve(); out.mkdir(parents=True, exist_ok=True)
RES = []
def check(cid, ok, d=""):
    RES.append({"scenario": "E1-enhanced-keys", "check": cid, "passed": bool(ok), "detail": str(d)[:400]})
    print(("PASS " if ok else "FAIL ") + cid, repr(str(d)[:200])); (out / "results.json").write_text(json.dumps(RES, indent=2))
def last(t):
    r = [l for l in t.screen().splitlines() if l.strip()]; return r[-1].strip()
fx = setup(Path("/tmp/v794"), nori, "E1")
klog = fx["run"] / "kitty.log"
wrap = Path(__file__).resolve().parent / "kittywrap.py"
t = term(fx, "E1", out / "E1", KITTY_LOG=str(klog))
t.run(f"python3 -I {wrap} nori --agent mock-model")
t.wait_for("Mock ACP options", 30); time.sleep(1)
check("keyboard_flags_pushed", "push" in klog.read_text(), klog.read_text())
slow_type(t, "count cedars")
t.capture("01-typed-enhanced")
check("typed.shift_enter_hint", last(t) == "Enter send · Shift+Enter newline", last(t))
t.keys("F12"); slow_type(t, "and pines")
t.capture("02-after-shift-enter")
rows = [l.strip() for l in t.screen().splitlines()]
check("shift_enter_inserts_newline", "› count cedars" in rows and "and pines" in rows, rows[-6:])
check("shift_enter_not_submitted", prompts(fx) == [], prompts(fx))
t.keys("C-j"); slow_type(t, "and firs")
t.capture("03-ctrl-j-too")
rows = [l.strip() for l in t.screen().splitlines()]
check("ctrl_j_still_newline", "and firs" in rows and prompts(fx) == [], rows[-6:])
t.keys("Enter"); time.sleep(3)
check("enter_sends_three_lines", prompts(fx) == ["count cedars\nand pines\nand firs"], prompts(fx))
t.close(); T.kill_server()
