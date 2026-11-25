"""Seed one session through `nori host new` and dump the host's replay from cursor 0."""
import json, subprocess, sys, time, shutil
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parent))
import fixtures, wire
nori = sys.argv[1]; run = Path(sys.argv[2])
shutil.rmtree(run, ignore_errors=True)
fx = fixtures.build(run)
env = fixtures.env_for(fx["homes"]["host"], {})
cwd = fx["roots"]["alpha"]
sh = lambda *a: subprocess.run([nori, *a], env=env, cwd=cwd, capture_output=True, text=True, timeout=60)
sh("host", "--port", "0")
try:
    sid = sh("host", "new", "--prompt", "first light").stdout.strip().splitlines()[-1]
    time.sleep(1.5)
    ep, tok = wire.host_endpoint(fx["homes"]["host"])
    w = wire.Wire(ep, tok, observe=False)
    r = w.call("_nori/session/subscribe", {"sessionId": sid, "cursor": 0})
    print("subscribe ->", json.dumps(r)[:300])
    for n in w.drain(2.0):
        p = n.get("params", {})
        ev = p.get("event", {})
        print(n.get("method"), p.get("seq"), json.dumps(ev)[:260])
        if len(sys.argv) > 3: open(sys.argv[3], "a").write(json.dumps(p) + "\n")
    w.close()
finally:
    sh("host", "stop")
