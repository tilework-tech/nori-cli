"""`nori attach` scenarios A1-A7 for #794 composer hints (synthetic orchard data)."""
import sys, time, json, re, subprocess
from pathlib import Path
from drv import *
nori = Path(sys.argv[1]).resolve(); out = Path(sys.argv[2]).resolve(); only = sys.argv[3].split(",") if len(sys.argv) > 3 and sys.argv[3] else None
out.mkdir(parents=True, exist_ok=True)
RES = []
def check(sid, cid, ok, detail=""):
    RES.append({"scenario": sid, "check": cid, "passed": bool(ok), "detail": str(detail)[:500]})
    print(("PASS " if ok else "FAIL ") + f"{sid}/{cid} {str(detail)[:220]!r}", flush=True)
    (out / "results.json").write_text(json.dumps(RES, indent=2))
def note(sid, cid, detail):
    RES.append({"scenario": sid, "check": cid, "passed": None, "detail": str(detail)[:500]})
    print(f"NOTE {sid}/{cid} {str(detail)[:220]!r}", flush=True)
    (out / "results.json").write_text(json.dumps(RES, indent=2))

def footer(t):
    rows = [l for l in t.screen().splitlines() if l.strip()]
    return rows[-1].strip() if rows else ""
def hints_part(f):
    m = re.split(r" · (?=(controlling|watching|reconnecting|RECONNECTING) ·)", f)
    return m[0] if len(m) > 1 else f
STATUS = re.compile(r"(controlling|watching) · ")

class Host:
    def __init__(self, sid, **envx):
        self.fx = setup(Path("/tmp/v794"), nori, sid)
        self.sid = sid; self.envx = envx; self.terms = []
        e = env(self.fx, **envx)
        r = subprocess.run([str(self.fx["bin"]/"nori"), "host", "--port", "0"], env=e, cwd=self.fx["proj"], capture_output=True, text=True, timeout=60)
        (out / sid).mkdir(parents=True, exist_ok=True); (out / sid / "host-start.txt").write_text(r.stdout + r.stderr)
        self.endpoint = json.loads((self.fx["home"]/"host"/"state.json").read_text())["endpoint"]
        r = subprocess.run([str(self.fx["bin"]/"nori"), "host", "new", "--cwd", str(self.fx["proj"])], env=e, cwd=self.fx["proj"], capture_output=True, text=True, timeout=60)
        self.session = r.stdout.strip().splitlines()[-1]
    def attach(self, name, cols=140, rows=26, remote=False):
        if remote:
            t = term(self.fx, f"{self.sid}-{name}", out / self.sid, cols=cols, rows=rows, cwd=self.fx["lproj"], home=self.fx["laptop"])
            t.run(f"nori attach --host {self.endpoint} --token-file {self.fx['home']/'host'/'token'} {self.session[:8]}")
        else:
            t = term(self.fx, f"{self.sid}-{name}", out / self.sid, cols=cols, rows=rows)
            t.run(f"nori attach {self.session[:8]}")
        t.wait_for(r"(controlling|watching) · ", 30, regex=True); time.sleep(1.2)
        self.terms.append(t); return t
    def close(self):
        for t in self.terms: t.close()
        subprocess.run([str(self.fx["bin"]/"nori"), "host", "stop"], env=env(self.fx), capture_output=True, timeout=60)

EMPTY_LOCAL = "/ commands · ! shell · @ files · ? help"

def a1_controller():
    sid = "A1-controller-steer"
    h = Host(sid, MOCK_AGENT_STEERING="1")
    a = h.attach("ctl", cols=180)
    a.capture("01-empty")
    f = footer(a)
    check(sid, "empty.hints_lead_status", f.startswith(EMPTY_LOCAL + " · controlling"), f)
    row = next(l for l in a.screen(ansi=True).splitlines() if "controlling" in re.sub(r"\x1b\[[0-9;]*m", "", l))
    check(sid, "empty.keys_bold", all(re.search(r"\x1b\[(0;)?1m(\x1b\[[0-9;]*m)*" + re.escape(k) + r"\x1b", row) for k in "/!@?"), repr(row[:200]))
    slow_type(a, "Sketch the orchard plan")
    a.capture("02-typed")
    check(sid, "typed.send_newline", hints_part(footer(a)) == "Enter send · Ctrl+J newline", footer(a))
    a.keys("Enter"); a.wait_for("Survey in progress.", 20); time.sleep(0.6)
    slow_type(a, "Count the cedar trees")
    a.capture("03-own-turn-typed")
    check(sid, "own_turn.steer_queue", hints_part(footer(a)) == "Enter steer · Tab queue · Ctrl+J newline", footer(a))
    a.keys("Tab"); time.sleep(1.2)
    a.capture("04-tab")
    check(sid, "tab.queued_not_sent", prompts(h.fx) == ["Sketch the orchard plan"], prompts(h.fx))
    # watcher during the controller's turn
    w = h.attach("watch", cols=180)
    w.capture("05-watcher-empty")
    fw = footer(w)
    check(sid, "watcher.empty_no_shell_no_enter", hints_part(fw) in ("/ commands · @ files · ? help", "/ commands · @ files"), fw)
    check(sid, "watcher.status_keeps_take_control", "alt+t take control" in fw, fw)
    slow_type(w, "Count the pine trees")
    w.capture("06-watcher-typed")
    fw = footer(w)
    check(sid, "watcher.typed_only_newline", hints_part(fw) == "Ctrl+J newline", fw)
    w.keys("Enter"); time.sleep(1.5)
    w.capture("07-watcher-enter")
    check(sid, "watcher.enter_sends_nothing", "Count the pine trees" not in prompts(h.fx), prompts(h.fx))
    check(sid, "watcher.draft_kept", "Count the pine trees" in w.screen(), w.screen()[-300:])
    w.keys("C-u")
    for _ in range(25): w.keys("BSpace")
    slow_type(w, "!")
    time.sleep(0.3); w.capture("08-watcher-bang")
    check(sid, "watcher.bang_no_enter", "Enter" not in hints_part(footer(w)), footer(w))
    w.keys("BSpace")
    for _ in range(3): w.keys("BSpace")
    # controller steers
    slow_type(a, "include the walnut row"); a.keys("Enter")
    a.wait_for("Survey correction received", 20); time.sleep(3)
    a.capture("09-steered")
    p = prompts(h.fx)
    check(sid, "steer.delivered_in_turn", "Survey correction received: include the walnut row" in a.screen() and "include the walnut row" not in p, p)
    check(sid, "queued.delivered_once_after", p.count("Count the cedar trees") == 1, p)
    check(sid, "idle.back_to_discovery", footer(a).startswith(EMPTY_LOCAL), footer(a))
    w.capture("10-watcher-after")
    h.close()

def a2_takeover_other_turn():
    sid = "A2-takeover-during-other-turn"
    h = Host(sid, MOCK_AGENT_STEERING="1")
    a = h.attach("first", cols=180)
    b = h.attach("second", cols=180)
    slow_type(a, "Sketch the orchard plan"); a.keys("Enter"); a.wait_for("Survey in progress.", 20)
    b.wait_for("Survey in progress.", 20)
    b.keys("M-t"); time.sleep(2)
    b.capture("01-took-control")
    check(sid, "second.now_controlling", "controlling ·" in footer(b), footer(b))
    slow_type(b, "Count the cedar trees")
    b.capture("02-typed-during-others-turn")
    fb = hints_part(footer(b))
    note(sid, "hint_while_turn_not_own", fb)
    b.keys("Enter"); time.sleep(2.5)
    b.capture("03-after-enter")
    a.capture("03b-first-after")
    p = prompts(h.fx)
    tail = "\n".join(b.screen().splitlines()[-12:])
    note(sid, "enter_outcome", {"prompts": p, "screen_tail": tail})
    check(sid, "hint_matches_outcome", (fb.startswith("Enter send") and ("Count the cedar trees" in p or "Queued" in tail or "queue" in tail.lower())) or not fb.startswith("Enter send"), (fb, p))
    # release the held turn
    slow_type(b, "/cancel"); b.keys("Escape"); b.keys("Enter"); time.sleep(3)
    b.capture("04-after-cancel")
    note(sid, "after_cancel_prompts", prompts(h.fx))
    h.close()

def a3_nonsteerable_paused():
    sid = "A3-nonsteerable-paused"
    h = Host(sid, MOCK_AGENT_STREAM_UNTIL_CANCEL="1")
    a = h.attach("ctl", cols=180)
    w = h.attach("watch", cols=180)
    slow_type(a, "Survey the pear rows"); a.keys("Enter"); time.sleep(1.5)
    slow_type(a, "then the plums")
    a.capture("01-own-turn-typed")
    fa = hints_part(footer(a))
    check(sid, "nonsteer.enter_queue_only", fa == "Enter queue · Ctrl+J newline", fa)
    a.keys("Enter"); time.sleep(1.5)
    a.capture("02-after-enter")
    check(sid, "enter.queued_not_delivered", prompts(h.fx) == ["Survey the pear rows"], prompts(h.fx))
    a.keys("Escape"); time.sleep(0.4)
    a.wait_for("paused", 15); time.sleep(1.0)
    a.capture("03-paused")
    w.capture("03w-watcher-paused")
    fa = hints_part(footer(a)); fw = hints_part(footer(w))
    check(sid, "paused.controller_enter_resume_first", fa.startswith("Enter resume queue · / commands"), fa)
    check(sid, "paused.watcher_no_resume", "resume" not in fw, fw)
    w.keys("Enter"); time.sleep(1.5)
    w.capture("04w-watcher-enter")
    check(sid, "paused.watcher_enter_does_not_resume", prompts(h.fx) == ["Survey the pear rows"], prompts(h.fx))
    note(sid, "watcher_enter_screen_tail", "\n".join(w.screen().splitlines()[-6:]))
    a.keys("Enter"); time.sleep(3)
    a.capture("05-resumed")
    check(sid, "paused.controller_enter_resumes_once", prompts(h.fx) == ["Survey the pear rows", "then the plums"], prompts(h.fx))
    h.close()

def a4_remote():
    sid = "A4-remote-shell"
    h = Host(sid, MOCK_AGENT_STEERING="1")
    a = h.attach("local", cols=180)
    r = h.attach("remote", cols=180, remote=True)
    r.keys("M-t"); time.sleep(2)
    r.capture("01-remote-empty")
    fr = footer(r)
    check(sid, "remote.controlling", "controlling ·" in fr, fr)
    note(sid, "remote.empty_hints", hints_part(fr))
    slow_type(r, "!")
    time.sleep(0.3)
    slow_type(r, "touch remote-canary-plum")
    r.capture("02-remote-bang-typed")
    fb = hints_part(footer(r))
    note(sid, "remote.bang_typed_hints", fb)
    r.keys("Enter"); time.sleep(3)
    r.capture("03-remote-bang-enter")
    canaries = sorted(str(p) for p in h.fx["run"].rglob("remote-canary-*"))
    ran = bool(canaries)
    shown = "! shell" in hints_part(fr)
    hint_line = next((l for l in r.screen().splitlines() if "emote shell commands" in l), "")
    note(sid, "remote.outcome", {"ran": ran, "canaries": canaries, "gray_hint": hint_line.strip()})
    check(sid, "remote.advertised_shell_matches_run", shown == ran, {"advertised": shown, "ran": ran})
    check(sid, "remote.enter_run_matches_run", fb.startswith("Enter run") == ran, {"hint": fb, "ran": ran})
    r.keys("C-u")
    for _ in range(30): r.keys("BSpace")
    slow_type(r, "@pea"); time.sleep(1.5)
    r.capture("04-remote-at")
    note(sid, "remote.at_files", {"advertised": "@ files" in hints_part(fr), "popup_has_host_file": "pear_ledger.rs" in r.screen()})
    check(sid, "remote.at_advertised_matches_popup", ("@ files" in hints_part(fr)) == ("pear_ledger.rs" in r.screen()), r.screen()[-300:])
    h.close()

def a5_narrow():
    sid = "A5-narrow"
    h = Host(sid)
    for cols in (40, 60, 80, 100, 140):
        t = h.attach(f"c{cols}", cols=cols)
        t.capture(f"{cols}-empty")
        f = footer(t)
        note(sid, f"{cols}.footer", f)
        check(sid, f"{cols}.status_kept", "controlling" in f or "watching" in f, f)
        slow_type(t, "count cedars"); t.capture(f"{cols}-typed")
        note(sid, f"{cols}.typed", footer(t))
        t.keys("C-d"); time.sleep(2)
    h.close()

def a6_enhanced():
    sid = "A6-attach-enhanced"
    h = Host(sid)
    klog = h.fx["run"] / "kitty.log"
    wrap = Path(__file__).resolve().parent / "kittywrap.py"
    t = term(h.fx, f"{sid}-k", out / sid, cols=160, rows=26, KITTY_LOG=str(klog))
    t.run(f"python3 -I {wrap} nori attach {h.session[:8]}")
    t.wait_for("controlling · ", 30); time.sleep(1); h.terms.append(t)
    slow_type(t, "count cedars")
    t.capture("01-typed")
    check(sid, "attach.shift_enter_hint", hints_part(footer(t)) == "Enter send · Shift+Enter newline", footer(t))
    t.keys("F12"); slow_type(t, "and pines")
    t.capture("02-shift-enter")
    rows = [l.strip() for l in t.screen().splitlines()]
    check(sid, "attach.shift_enter_newline_not_sent", "and pines" in rows and prompts(h.fx) == [], (rows[-5:], prompts(h.fx)))
    h.close()

ALL = {"A1": a1_controller, "A2": a2_takeover_other_turn, "A3": a3_nonsteerable_paused, "A4": a4_remote, "A5": a5_narrow, "A6": a6_enhanced}
if __name__ == "__main__":
    for k, fn in ALL.items():
        if only and k not in only: continue
        try: fn()
        except Exception as e: check(k, "exception", False, repr(e)[:400])
    T.kill_server()
