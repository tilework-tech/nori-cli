import sys, time, json
from pathlib import Path
from drv import *
nori = Path(sys.argv[1]).resolve(); out = Path(sys.argv[2]).resolve()
fx = setup(Path("/tmp/v794"), nori, "P9-tab-nonsteer")
t = term(fx, "P9", out / "P9-tab-nonsteer", MOCK_AGENT_DELAY_MS="6000")
t.run("nori --agent mock-model"); t.wait_for("Mock ACP options", 30); time.sleep(1)
slow_type(t, "Survey the pear rows"); t.keys("Enter"); time.sleep(1.2)
slow_type(t, "then the plums"); t.keys("Tab"); time.sleep(1.0); t.capture("01-tab")
queued_now = prompts(fx) == ["Survey the pear rows"] and "then the plums" in t.screen()
time.sleep(14)
r = {"tab_queued_not_sent": queued_now, "prompts_after": prompts(fx)}
print(json.dumps(r)); (out / "P9-tab-nonsteer" / "result.json").write_text(json.dumps(r, indent=2))
t.close(); T.kill_server()
