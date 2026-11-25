import sys, time, json
from pathlib import Path
from drv import *
nori = Path(sys.argv[1]).resolve(); out = Path(sys.argv[2]).resolve()
fx = setup(Path("/tmp/v794"), nori, "P8-busy-bang")
t = term(fx, "P8", out / "P8-busy-bang", MOCK_AGENT_STEERING="1")
t.run("nori --agent mock-model"); t.wait_for("Mock ACP options", 30); time.sleep(1)
slow_type(t, "Sketch the orchard plan"); t.keys("Enter"); t.wait_for("Survey in progress.", 15); time.sleep(0.5)
rows = [l for l in t.screen().splitlines() if l.strip()]; busy = rows[-1].strip()
slow_type(t, "!echo busy-$((2+3))"); t.capture("01-busy-bang-typed")
rows = [l for l in t.screen().splitlines() if l.strip()]; typed = rows[-1].strip()
t.keys("Enter"); time.sleep(2.5); t.capture("02-busy-bang-ran")
r = {"busy_empty_hint": busy, "typed_hint": typed, "ran": "busy-5" in t.screen(), "prompts": prompts(fx)}
print(json.dumps(r)); (out / "P8-busy-bang" / "result.json").write_text(json.dumps(r, indent=2))
t.close(); T.kill_server()
