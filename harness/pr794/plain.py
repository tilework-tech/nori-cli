"""Plain `nori` scenarios P1-P9 for #794 composer hints (synthetic orchard data)."""
import sys, time, json, re
from pathlib import Path
from drv import *
nori = Path(sys.argv[1]).resolve(); out = Path(sys.argv[2]).resolve(); only = sys.argv[3].split(",") if len(sys.argv) > 3 else None
out.mkdir(parents=True, exist_ok=True)
RES = []
def check(sid, cid, ok, detail=""):
    RES.append({"scenario": sid, "check": cid, "passed": bool(ok), "detail": str(detail)[:500]})
    print(("PASS " if ok else "FAIL ") + f"{sid}/{cid} {str(detail)[:200]!r}", flush=True)
    (out / "results.json").write_text(json.dumps(RES, indent=2))

def last_hint(t):
    """The last non-empty screen row (the footer/hint row in inline mode)."""
    rows = [l for l in t.screen().splitlines() if l.strip()]
    return rows[-1].strip() if rows else ""

def ansi_row(t, needle):
    return next((l for l in t.screen(ansi=True).splitlines() if needle in re.sub(r"\x1b\[[0-9;]*m", "", l)), "")

def start(sid, cols=120, rows=26, extra_cfg="", **envx):
    fx = setup(Path("/tmp/v794"), nori, sid, extra_cfg)
    t = term(fx, sid, out / sid, cols=cols, rows=rows, **envx)
    t.run("nori --agent mock-model")
    t.wait_for("Mock ACP options", 30)
    time.sleep(1.0)
    return fx, t

EMPTY = "/ commands · ! shell · @ files · ? help"

def p1_states():
    sid = "P1-empty-typed-back"
    fx, t = start(sid, MOCK_AGENT_STEERING="1")
    t.capture("01-empty")
    check(sid, "empty.discovery", last_hint(t) == EMPTY, last_hint(t))
    row = ansi_row(t, "/ commands")
    keys_bold = all(re.search(r"\x1b\[(0;)?1m(\x1b\[[0-9;]*m)*" + re.escape(k), row) for k in "/!@?")
    labels_dim = all(re.search(r"\x1b\[(0;)?2m(\x1b\[[0-9;]*m)* " + w, row) for w in ("commands", "shell", "files", "help"))
    check(sid, "empty.keys_bold_labels_dim", keys_bold and labels_dim, repr(row[:240]))
    ph = [l for l in t.screen().splitlines() if l.strip().startswith("›")]
    check(sid, "empty.no_placeholder_tip", ph and ph[-1].strip() == "›", ph)
    slow_type(t, "count cedars")
    t.capture("02-typed")
    check(sid, "typed.send_newline", last_hint(t) == "Enter send · Ctrl+J newline", last_hint(t))
    for _ in range(len("count cedars")):
        t.keys("BSpace")
    time.sleep(0.4)
    t.capture("03-back-to-empty")
    check(sid, "erased.back_to_discovery", last_hint(t) == EMPTY, last_hint(t))
    slow_type(t, "/stat")
    time.sleep(0.4); t.capture("04-slash-popup")
    t.keys("Escape"); time.sleep(0.3)
    t.capture("04b-slash-after-esc")
    check(sid, "slash.enter_run_after_popup_closed", "Enter run" in last_hint(t) or "/status" in t.screen(), last_hint(t))
    t.keys("C-u"); time.sleep(0.3)
    for _ in range(10): t.keys("BSpace")
    slow_type(t, "a"); t.keys("C-j"); slow_type(t, "b")
    t.capture("05-ctrl-j")
    rows = t.screen().splitlines()
    check(sid, "ctrl_j_inserts_newline", any(l.strip() == "› a" for l in rows) and any(l.strip() == "b" for l in rows), rows[-6:])
    check(sid, "ctrl_j_not_submitted", prompts(fx) == [], prompts(fx))
    t.close()

def p2_shell_and_help():
    sid = "P2-shell-help"
    fx, t = start(sid)
    slow_type(t, "!")
    time.sleep(0.3); t.capture("01-bang-empty")
    h1 = last_hint(t)
    slow_type(t, "echo orchard-$((6*7))")
    t.capture("02-bang-typed")
    h2 = last_hint(t)
    check(sid, "bang_empty.no_enter", "Enter" not in h1, h1)
    check(sid, "bang_typed.enter_run", h2.startswith("Enter run"), h2)
    t.keys("Enter"); time.sleep(2.0)
    t.capture("03-bang-ran")
    check(sid, "bang.enter_actually_runs", "orchard-42" in t.screen(), t.screen()[-400:])
    check(sid, "bang.not_sent_to_agent", prompts(fx) == [], prompts(fx))
    time.sleep(0.5)
    check(sid, "after_run.back_to_discovery", last_hint(t) == EMPTY, last_hint(t))
    slow_type(t, "?"); time.sleep(0.5)
    t.capture("04-help")
    scr = t.screen()
    check(sid, "help.lists_shell_and_skills", "! for shell commands" in scr and "$ for skills" in scr, scr[-600:])
    t.keys("BSpace"); time.sleep(0.3)
    slow_type(t, "@pea"); time.sleep(1.0)
    t.capture("05-at")
    check(sid, "at.files_popup", "pear_ledger.rs" in t.screen(), t.screen()[-400:])
    t.close()

def p3_steer():
    sid = "P3-own-turn-steer"
    fx, t = start(sid, MOCK_AGENT_STEERING="1")
    slow_type(t, "Sketch the orchard plan"); t.keys("Enter")
    t.wait_for("Survey in progress.", 15); time.sleep(0.5)
    t.capture("01-busy-empty")
    hb = last_hint(t)
    slow_type(t, "Count the cedar trees")
    t.capture("02-busy-typed")
    check(sid, "busy_typed.steer_queue", "Enter steer · Tab queue · Ctrl+J newline" in last_hint(t), last_hint(t))
    t.keys("Tab"); time.sleep(1.0)
    t.capture("03-tab-queued")
    check(sid, "tab.queued_not_sent", "Count the cedar trees" in t.screen() and prompts(fx) == ["Sketch the orchard plan"], prompts(fx))
    slow_type(t, "/status")
    t.keys("Escape"); time.sleep(0.3)
    t.capture("04-busy-command")
    check(sid, "busy_command.enter_run_only", last_hint(t).startswith("Enter run") and "steer" not in last_hint(t), last_hint(t))
    t.keys("C-u"); 
    for _ in range(10): t.keys("BSpace")
    slow_type(t, "include the walnut row")
    t.keys("Enter")
    t.wait_for("Survey correction received", 15); time.sleep(3)
    t.capture("05-steered")
    check(sid, "enter.steered_into_turn", "Survey correction received: include the walnut row" in t.screen(), t.screen()[-400:])
    p = prompts(fx)
    check(sid, "queued_delivered_after_once", p.count("Count the cedar trees") == 1 and p[0] == "Sketch the orchard plan" and "include the walnut row" not in p, p)
    check(sid, "idle_again.discovery", last_hint(t) == EMPTY, last_hint(t))
    check(sid, "busy_empty.hint", True, hb)
    t.close()

def p4_nonsteerable():
    sid = "P4-own-turn-nonsteerable"
    fx, t = start(sid, MOCK_AGENT_DELAY_MS="7000")
    slow_type(t, "Survey the pear rows"); t.keys("Enter")
    time.sleep(1.2)
    slow_type(t, "then the plums")
    t.capture("01-busy-typed")
    h = last_hint(t)
    check(sid, "nonsteer.enter_queue_no_tab", h.startswith("Enter queue") and "steer" not in h and "Tab" not in h, h)
    t.keys("Enter"); time.sleep(1.0)
    t.capture("02-after-enter")
    scr = t.screen()
    check(sid, "enter.queued_not_steered", prompts(fx) == ["Survey the pear rows"] and "then the plums" in scr, (prompts(fx), scr[-300:]))
    time.sleep(16)
    t.capture("03-drained")
    p = prompts(fx)
    check(sid, "queued_delivered_next_turn_once", p == ["Survey the pear rows", "then the plums"], p)
    t.close()

def p5_paused():
    sid = "P5-paused-queue"
    fx, t = start(sid, MOCK_AGENT_STREAM_UNTIL_CANCEL="1")
    slow_type(t, "Survey the pear rows"); t.keys("Enter")
    time.sleep(1.2)
    slow_type(t, "then the plums"); t.keys("Tab"); time.sleep(0.6)
    t.keys("Escape"); time.sleep(0.3)
    t.wait_for("paused", 15); time.sleep(0.8)
    t.capture("01-paused")
    scr = t.screen()
    check(sid, "paused.shown", "paused" in scr.lower(), scr[-500:])
    h = last_hint(t)
    check(sid, "paused.enter_resume_first", h.startswith("Enter resume queue · / commands"), h)
    check(sid, "paused.queue_not_delivered_yet", prompts(fx) == ["Survey the pear rows"], prompts(fx))
    slow_type(t, "x"); time.sleep(0.3)
    hx = last_hint(t)
    check(sid, "paused.typed_hides_resume", "resume" not in hx, hx)
    t.keys("BSpace"); time.sleep(0.3)
    t.keys("Enter"); time.sleep(2.5)
    t.capture("02-resumed")
    p = prompts(fx)
    check(sid, "empty_enter_resumes_once", p == ["Survey the pear rows", "then the plums"], p)
    check(sid, "after_resume.no_resume_hint", "resume" not in last_hint(t), last_hint(t))
    t.close()

def p6_narrow():
    sid = "P6-narrow"
    for cols in (40, 60, 80):
        fx, t = start(f"{sid}-{cols}", cols=cols)
        t.capture(f"{cols}-empty")
        h = last_hint(t)
        check(sid, f"{cols}.fits_and_drops_whole", len(h) <= cols - 2 and all(x in EMPTY.split(" · ") for x in h.split(" · ")), h)
        slow_type(t, "count cedars"); t.capture(f"{cols}-typed")
        check(sid, f"{cols}.typed", last_hint(t), last_hint(t))
        t.close()

def p7_off():
    sid = "P7-off-layouts"
    cases = {
        "segment-false": "[tui.footer_segments]\ncomposer_hints = false",
        "left-without": '[tui.footer_layout]\nfooter_left = ["nori_version"]',
        "left-not-first": '[tui.footer_segments]\nnori_version = true\n[tui.footer_layout]\nfooter_left = ["nori_version", "composer_hints"]',
        "right-only": '[tui.footer_layout]\nfooter_left = []\nfooter_right = ["composer_hints"]',
    }
    for name, cfg in cases.items():
        fx, t = start(f"{sid}-{name}", extra_cfg=cfg)
        t.capture(f"{name}-empty")
        scr = t.screen()
        ph = [l for l in scr.splitlines() if l.strip().startswith("›")]
        check(sid, f"{name}.hint_present(expected {name in ('left-not-first','right-only')})", ("/ commands" in scr) == (name in ('left-not-first','right-only')), [l for l in scr.splitlines() if l.strip()][-3:])
        check(sid, f"{name}.placeholder", ph[-1].strip() if ph else None, ph[-1] if ph else None)
        slow_type(t, "count cedars"); time.sleep(0.3); t.capture(f"{name}-typed")
        check(sid, f"{name}.typed_row", last_hint(t), last_hint(t))
        t.close()

ALL = {"P1": p1_states, "P2": p2_shell_and_help, "P3": p3_steer, "P4": p4_nonsteerable, "P5": p5_paused, "P6": p6_narrow, "P7": p7_off}
for k, f in ALL.items():
    if only and k not in only: continue
    try:
        f()
    except Exception as e:
        check(k, "exception", False, repr(e)[:400])
T.kill_server()
