import json, sys, time
from pathlib import Path
sys.path.insert(0, sys.argv[2]); sys.path.insert(0, str(Path(sys.argv[2]).parent))
from kiln import KilnHost
import wire
with KilnHost(sys.argv[1], 8) as h:
    time.sleep(1)
    ep, tok = wire.host_endpoint(h.home)
    w = wire.Wire(ep, tok, observe=False)
    w.call("_nori/session/subscribe", {"session_id": h.session_id, "cursors": {h.session_id: 0}})
    prev = None
    for n in w.drain(2.0):
        ev = json.dumps(n.get("params", {}))
        kind = "plan" if '"plan"' in ev else ("mark" if "KilnMark" in ev else ("tool" if "tool_call" in ev else ("user" if "user_message" in ev else ("agent" if "agent_message" in ev else "other"))))
        tag = kind + (":" + ev[ev.find("KilnMark"):ev.find("KilnMark")+12] if kind=="mark" else "") + (":"+ev[ev.find("Record batch"):ev.find("Record batch")+14] if kind=="plan" else "")
        if tag != prev: print(tag)
        prev = tag
    w.close()
