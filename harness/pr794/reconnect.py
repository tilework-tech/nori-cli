"""A7: hints while reconnecting through a dropped relay (synthetic)."""
import sys, time
from pathlib import Path
sys.argv += [""] if len(sys.argv) < 4 else []
import attach as A
from proxy import Proxy
from drv import slow_type, prompts
def a7():
    sid = "A7-reconnecting"
    h = A.Host(sid)
    p = Proxy(h.endpoint)
    t = A.term(h.fx, f"{sid}-r", A.out / sid, cols=180, rows=26, cwd=h.fx["lproj"], home=h.fx["laptop"])
    t.run(f"nori attach --host {p.url} --token-file {h.fx['home']/'host'/'token'} {h.session[:8]}")
    t.wait_for("controlling · ", 30); time.sleep(1); h.terms.append(t)
    slow_type(t, "count cedars")
    p.drop_all(hold=8.0)
    t.wait_for(r"(?i)reconnecting", 10, regex=True); time.sleep(0.5)
    t.capture("01-reconnecting-typed")
    f = A.footer(t)
    A.check(sid, "reconnecting.no_send_key", "Enter" not in A.hints_part(f) and "Ctrl+J newline" in f, f)
    t.keys("Enter"); time.sleep(1)
    A.check(sid, "reconnecting.enter_sends_nothing", prompts(h.fx) == [], prompts(h.fx))
    t.wait_for("controlling · ", 20); time.sleep(1.5)
    t.capture("02-reconnected")
    f = A.footer(t)
    A.check(sid, "reconnected.send_back", A.hints_part(f).startswith("Enter send"), f)
    p.close(); h.close()
a7()
A.T.kill_server()
