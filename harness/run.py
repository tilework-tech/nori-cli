#!/usr/bin/env python3
"""Run the host TUI acceptance matrix against one nori binary.

  python3 -I harness/run.py --nori PATH --out results/<sha8> [--only S03,S07]

Each scenario builds a fresh synthetic run directory, drives real tmux PTYs,
watches the host through an independent observer connection and the
adapter ledger, then records checks in results.json. Screens are saved as
.txt and .ansi; render PNGs afterwards with harness/render.sh.
"""
import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import time
import traceback
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import fixtures  # noqa: E402
import term  # noqa: E402
import wire  # noqa: E402
from proxy import Proxy  # noqa: E402

P = argparse.ArgumentParser()
P.add_argument("--nori", required=True)
P.add_argument("--out", required=True)
P.add_argument("--only", default="")
P.add_argument("--merge", action="store_true", help="keep results.json rows of scenarios not re-run")
ARGS = P.parse_args()
# Terminals run `nori` from a synthetic bin directory so captures and logs
# show only synthetic paths.
# ACC_LANE runs several commits side by side, each with its own work dir and tmux socket.
WORK = Path("/tmp/nori-host-tui-acceptance" + (f"-{os.environ['ACC_LANE']}" if os.environ.get("ACC_LANE") else ""))
BIN = WORK / "bin"
BIN.mkdir(parents=True, exist_ok=True)
(BIN / "nori").unlink(missing_ok=True)
(BIN / "nori").symlink_to(Path(ARGS.nori).resolve())
NORI = "nori"
OUT = Path(ARGS.out).resolve()
OUT.mkdir(parents=True, exist_ok=True)
RESULTS = []
if ARGS.merge and (OUT / "results.json").exists():
    rerun = {x for x in ARGS.only.split(",") if x}
    RESULTS = [r for r in json.loads((OUT / "results.json").read_text()) if r["scenario"] not in rerun]
HINT_TOKENS = re.compile(r"(alt\+t|alt\+s|alt\+↑|alt\+↓|alt\+up|alt\+down|ctrl\+d)", re.I)


class Ctx:
    def __init__(self, sid, **fixture_args):
        self.sid = sid
        self.dir = OUT / sid
        if self.dir.exists():
            shutil.rmtree(self.dir)
        self.run_dir = WORK / sid
        if self.run_dir.exists():
            shutil.rmtree(self.run_dir)
        self.fx = fixtures.build(self.run_dir, **fixture_args)
        self.terms, self.host_up = [], False

    # -- host --------------------------------------------------------------
    def env(self, home="host", **extra):
        env = fixtures.env_for(self.fx["homes"][home], extra)
        env["PATH"] = f"{BIN}:{env['PATH']}"
        return env

    def nori(self, *args, home="host", cwd=None):
        return subprocess.run([str(BIN / "nori"), *args], env=self.env(home), cwd=cwd or self.fx["roots"]["alpha"],
                              capture_output=True, text=True, timeout=60)

    def start_host(self, cwd="alpha"):
        out = self.nori("host", "--port", "0", cwd=self.fx["roots"][cwd])
        (self.dir / "host-start.txt").parent.mkdir(parents=True, exist_ok=True)
        (self.dir / "host-start.txt").write_text(out.stdout + out.stderr)
        self.host_up = True
        return wire.host_endpoint(self.fx["homes"]["host"])

    def observer(self):
        endpoint, token = wire.host_endpoint(self.fx["homes"]["host"])
        return wire.Wire(endpoint, token, observe=True, log=self.dir / "observer-wire.jsonl")

    def term(self, name, cwd="alpha", home="host", cols=120, rows=36):
        t = term.Term(f"{self.sid}-{name}", self.env(home), self.fx["roots"][cwd], self.dir, cols, rows)
        self.terms.append(t)
        return t

    def ledger(self, kind=None, method=None):
        rows = fixtures.read_ledger(self.fx["ledger"])
        return [r for r in rows if (kind is None or r["kind"] == kind)
                and (method is None or r.get("method") == method)]

    def ticks(self):
        return len(self.ledger("tick"))

    # -- checks --------------------------------------------------------------
    def check(self, cid, passed, detail=""):
        RESULTS.append({"scenario": self.sid, "check": cid, "passed": bool(passed), "detail": str(detail)[:600]})
        print(("PASS " if passed else "FAIL ") + f"{self.sid}/{cid} {str(detail)[:160]}", flush=True)
        (OUT / "results.json").write_text(json.dumps(RESULTS, indent=2))

    def finish(self):
        for t in self.terms:
            t.close()
        if self.host_up:
            self.nori("host", "stop")
        self.dir.mkdir(parents=True, exist_ok=True)
        shutil.copy(self.fx["ledger"], self.dir / "ledger.jsonl")


# ---------------------------------------------------------------------------
# helpers that tolerate both today's attach and the shared TUI
# ---------------------------------------------------------------------------

def hint_rows(text):
    return [l for l in text.splitlines() if len(HINT_TOKENS.findall(l)) >= 2]


def last_nonblank(text):
    lines = [l for l in text.splitlines() if l.strip()]
    return lines[-1] if lines else ""


def wait_composer(t, timeout=20):
    return t.wait_for("›", timeout)


def attach_new_session(t, folder=None):
    """Open a session from a fresh attach. Works for the picker or a composer."""
    screen = t.wait_for(r"(Sessions|›)", 20, regex=True)
    if "No sessions yet" in screen or "Sessions" in screen:
        t.keys("Escape")
    t.type("/new" + (f" {folder}" if folder else ""))
    t.keys("Enter")
    time.sleep(1.5)
    if re.search(r"^\s*/new\b", t.screen(), re.M):
        t.keys("Enter")  # a slash popup may consume the first Enter
    t.wait_for(r"(ready|Ready|Send a prompt|›)", 20, regex=True)


def running(pid):
    """A process that still runs; an unreaped zombie has already exited."""
    try:
        return Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()[0] != "Z"
    except (FileNotFoundError, ProcessLookupError):
        return False


def wait_ticks(ctx, n, timeout=20):
    deadline = time.time() + timeout
    while time.time() < deadline:
        if ctx.ticks() >= n:
            return
        time.sleep(0.2)
    raise TimeoutError(f"adapter produced {ctx.ticks()} ticks, wanted {n}")


def tty_sane(text):
    flags = text.replace("\n", " ")
    return " icanon" in flags and " echo " in flags and "-icanon" not in flags and "-echo " not in flags


def check_restored(ctx, t, prefix):
    deadline = time.time() + 10
    while time.time() < deadline and not t.at_shell():
        time.sleep(0.2)
    ctx.check(f"{prefix}.shell_prompt_returns", t.at_shell(), last_nonblank(t.screen()))
    ctx.check(f"{prefix}.alt_screen_off", not t.alt_screen())
    ctx.check(f"{prefix}.cursor_visible", t.cursor_visible())
    counts = term.escape_counts(t.raw())
    ctx.check(f"{prefix}.escapes_balanced",
              counts["alt_on"] == counts["alt_off"] and counts["paste_on"] == counts["paste_off"]
              and counts["focus_on"] == counts["focus_off"] and counts["kbd_push"] <= counts["kbd_pop"],
              counts)
    ctx.check(f"{prefix}.tty_cooked", tty_sane(t.tty_modes()))
    return counts


# ---------------------------------------------------------------------------
# scenarios
# ---------------------------------------------------------------------------

def s01_foreground_launch_exit(ctx):
    """Plain nori: inline, owns its sessions, exits cleanly, no host left behind."""
    t = ctx.term("fg")
    t.run("echo before-nori-marker")
    t.run(f"{NORI} --skip-trust-directory")
    wait_composer(t)
    ctx.check("inline.no_alt_screen", not t.alt_screen())
    t.type("hello lantern")
    t.keys("Enter")
    t.wait_for("heard: hello lantern", 20)
    screen = t.screen()
    rows = hint_rows(screen)
    ctx.check("hint_row.exactly_one", len(rows) == 1, rows)
    ctx.check("hint_row.at_bottom", bool(rows) and rows[-1] == last_nonblank(screen), last_nonblank(screen))
    t.capture("fg-running")
    t.type("/quit")
    t.keys("Enter")
    time.sleep(0.6)
    if not t.at_shell():
        t.keys("Enter")
    check_restored(ctx, t, "exit")
    history = t.history()
    ctx.check("inline.transcript_in_scrollback", "heard: hello lantern" in history
              and history.find("before-nori-marker") < history.find("heard: hello lantern"))
    t.capture("fg-after-exit")
    time.sleep(1)
    host_state = ctx.fx["homes"]["host"] / "host" / "state.json"
    ls = ctx.nori("host", "ls")
    ctx.check("lifetime.no_host_outlives_terminal", ls.returncode != 0 or "working" not in ls.stdout,
              (ls.stdout + ls.stderr)[-300:] + f" state.json={host_state.exists()}")
    spawned = {r["pid"] for r in ctx.ledger("spawn")}
    alive = [p for p in spawned if running(p)]
    ctx.check("lifetime.adapters_stopped", not alive, alive)


def s02_foreground_quit_stops_turn(ctx):
    """Plain nori keeps foreground lifetime: quitting mid-turn stops the turn."""
    t = ctx.term("fg")
    t.run(f"{NORI} --skip-trust-directory")
    wait_composer(t)
    t.type("hold the lamp")
    t.keys("Enter")
    wait_ticks(ctx, 3)
    t.keys("C-c")
    time.sleep(0.4)
    t.type("/quit")
    t.keys("Enter")
    time.sleep(1)
    if not t.at_shell():
        t.keys("Enter")
    check_restored(ctx, t, "exit")
    before = ctx.ticks()
    time.sleep(1.5)
    after = ctx.ticks()
    ctx.check("lifetime.turn_stopped_after_quit", after == before, f"ticks {before}->{after}")


def s03_attach_detach_keeps_turn(ctx, how):
    """nori attach: alternate screen, detach by Ctrl+D, /quit or PTY hangup; turn survives."""
    ctx.start_host()
    t = ctx.term("attach")
    t.run("echo before-attach-marker")
    t.run(f"{NORI} attach")
    attach_new_session(t)
    ctx.check("attach.alt_screen_on", t.alt_screen())
    t.type("hold the lamp")
    t.keys("Enter")
    t.wait_for("tick 3", 20)
    screen = t.screen()
    rows = hint_rows(screen)
    ctx.check("hint_row.exactly_one", len(rows) == 1, rows)
    ctx.check("hint_row.at_bottom", bool(rows) and rows[-1] == last_nonblank(screen), last_nonblank(screen))
    t.capture("attach-working")
    if how == "ctrl-d":
        t.keys("C-d")
    elif how == "quit":
        t.type("/quit")
        t.keys("Enter")
        time.sleep(0.5)
        if not t.at_shell() and t.alt_screen():
            t.keys("Enter")
    if how == "hangup":
        pane_pid = t.fmt("#{pane_pid}")
        attach_pids = subprocess.run(["pgrep", "-P", pane_pid], capture_output=True, text=True).stdout.split()
        t.close()
        time.sleep(1.5)
        ctx.check("detach.client_exits_on_hangup", not any(running(p) for p in attach_pids), attach_pids)
    else:
        counts = check_restored(ctx, t, "detach")
        history = t.history()
        ctx.check("alt.scrollback_untouched", "tick" not in history and "before-attach-marker" in history)
        t.capture("attach-after-detach")
    before = ctx.ticks()
    time.sleep(1.5)
    after = ctx.ticks()
    ctx.check("detach.turn_keeps_running", after > before, f"ticks {before}->{after}")
    obs = ctx.observer()
    sessions = obs.sessions()
    ctx.check("detach.session_still_on_host", len(sessions) == 1, sessions)
    obs.close()
    ls = ctx.nori("host", "ls")
    ctx.check("detach.host_reports_working", "working" in ls.stdout, ls.stdout)
    if how == "ctrl-d":
        t2 = ctx.term("reattach")
        t2.run(f"{NORI} attach {sessions[0]['sessionId'][:8]}")
        t2.wait_for("tick", 20)
        n1 = max(map(int, re.findall(r"tick (\d+)", t2.screen())) or [0])
        time.sleep(1.2)
        n2 = max(map(int, re.findall(r"tick (\d+)", t2.screen())) or [0])
        ctx.check("reattach.sees_live_turn", n2 > n1 > 0, f"{n1}->{n2}")
        t2.capture("reattach-live")


def s04_close_ends_session(ctx):
    """/close ends the session on the host and in the adapter."""
    ctx.start_host()
    t = ctx.term("attach")
    t.run(f"{NORI} attach")
    attach_new_session(t)
    t.type("first light")
    t.keys("Enter")
    t.wait_for("heard: first light", 20)
    obs = ctx.observer()
    before = obs.sessions()
    t.type("/close")
    t.keys("Enter")
    time.sleep(0.6)
    if re.search(r"^\s*/close\b", t.screen(), re.M):
        t.keys("Enter")
    time.sleep(2)
    after = obs.sessions()
    live = [s for s in after if (s.get("_meta") or {}).get("nori/session", {}).get("live")]
    ctx.check("close.session_not_live_on_host", len(before) == 1 and not live, {"before": before, "after": after})
    adapter_pids = {r["pid"] for r in ctx.ledger("spawn")}
    deadline = time.time() + 8
    while time.time() < deadline and not (ctx.ledger("request", "session/close") or ctx.ledger("stdin_eof")):
        time.sleep(0.25)
    ctx.check("close.adapter_released_session",
              bool(ctx.ledger("request", "session/close")) or bool(ctx.ledger("stdin_eof"))
              or not any(running(p) for p in adapter_pids),
              {"tail": [r["kind"] + ":" + r.get("method", "") for r in ctx.ledger()][-3:],
               "adapter_running": [p for p in adapter_pids if running(p)]})
    ctx.check("close.client_still_attached", t.alt_screen() and not t.at_shell())
    t.capture("after-close")
    obs.close()


def s05_slash_discovery(ctx):
    """The attach `/` menu matches the normal TUI; unsupported items are greyed with a reason."""
    fg = ctx.term("fg")
    fg.run(f"{NORI} --skip-trust-directory")
    wait_composer(fg)
    fg.type("/")
    time.sleep(1)
    normal = fg.screen()
    fg.capture("normal-slash-menu")
    fg.keys("BSpace")
    ctx.start_host()
    t = ctx.term("attach")
    t.run(f"{NORI} attach")
    attach_new_session(t)
    t.type("/")
    time.sleep(1.2)
    menu = t.screen()
    t.capture("attach-slash-menu")
    expected = ["/agent", "/model", "/config", "/approvals", "/new", "/sessions", "/delegate"]
    missing = [c for c in expected if c not in menu]
    ctx.check("slash.menu_opens_with_normal_commands", not missing, f"missing {missing}")
    t.keys("BSpace")
    for probe in ["/diff", "/browse", "/undo"]:
        t.type(probe)
        time.sleep(0.8)
        screen = t.screen()
        row = next((l for l in screen.splitlines() if l.strip().startswith(probe)), "")
        ctx.check(f"slash.{probe[1:]}_listed_with_reason",
                  bool(row) and re.search(r"(not available|unavailable|host)", row, re.I), row)
        t.keys("C-u")
    t.type("/diff")
    t.keys("Enter")
    time.sleep(1.5)
    prompts = [r.get("text", "") for r in ctx.ledger("request", "session/prompt")]
    ctx.check("slash.unavailable_not_sent_as_prompt", not any("/diff" in p for p in prompts), prompts)
    t.keys("C-u")


def s06_cwd_inheritance(ctx):
    """/new follows the foreground folder; /new DIR overrides; empty host uses the host folder."""
    ctx.start_host(cwd="alpha")
    t = ctx.term("attach", cwd="laptop")
    endpoint, _ = wire.host_endpoint(ctx.fx["homes"]["host"])
    token_file = ctx.fx["homes"]["host"] / "host" / "token"
    t.run(f"{NORI} attach --host {endpoint} --token-file {token_file}")
    attach_new_session(t)
    news = ctx.ledger("request", "session/new")
    ctx.check("cwd.empty_remote_falls_back_to_host_folder",
              bool(news) and news[-1].get("cwd") == str(ctx.fx["roots"]["alpha"]), news[-1:] )
    t.type(f"/new {ctx.fx['roots']['beta']}")
    t.keys("Enter")
    time.sleep(2)
    news = ctx.ledger("request", "session/new")
    ctx.check("cwd.explicit_override", news[-1].get("cwd") == str(ctx.fx["roots"]["beta"]), news[-1])
    t.type("/new")
    t.keys("Enter")
    time.sleep(0.6)
    if re.search(r"^\s*/new\b", t.screen(), re.M):
        t.keys("Enter")
    time.sleep(2)
    news = ctx.ledger("request", "session/new")
    ctx.check("cwd.plain_new_inherits_foreground", news[-1].get("cwd") == str(ctx.fx["roots"]["beta"]), news[-1])
    screen = t.screen()
    ctx.check("cwd.confirmation_names_folder", "beta" in screen,
              [l for l in screen.splitlines() if "app" in l])
    t.capture("after-inherited-new")
    t.type("/new assets/")
    t.keys("Tab")
    time.sleep(1)
    rows = [l for l in t.screen().splitlines() if "/new" in l or "-only" in l]
    ctx.check("cwd.dir_completion_from_session_fs",
              any("beta-only" in l for l in rows) and not any("laptop-only" in l for l in rows), rows)
    t.capture("new-dir-completion")
    t.keys("C-u")


def s07_remote_at_completion(ctx):
    """`@` suggests the foreground session's files on the host, never the laptop's, and never stale."""
    ctx.start_host(cwd="alpha")
    endpoint, _ = wire.host_endpoint(ctx.fx["homes"]["host"])
    token_file = ctx.fx["homes"]["host"] / "host" / "token"
    t = ctx.term("attach", cwd="laptop", home="laptop")
    t.run(f"{NORI} attach --host {endpoint} --token-file {token_file}")
    attach_new_session(t, str(ctx.fx["roots"]["alpha"]))
    t.type("@lan")
    time.sleep(2)
    alpha_screen = t.screen()
    t.capture("at-alpha")
    ctx.check("at.alpha_files_offered", "lantern_wick" in alpha_screen, alpha_screen[-400:])
    ctx.check("at.no_laptop_files", "decoy_laptop" not in alpha_screen)
    t.keys("C-u")
    t.type("@decoy")
    time.sleep(2)
    ctx.check("at.laptop_file_not_found", "decoy_laptop" not in t.screen())
    t.keys("C-u")
    t.type(f"/new {ctx.fx['roots']['beta']}")
    t.keys("Enter")
    time.sleep(2)
    t.type("@")
    t.type("src/")
    time.sleep(2)
    beta_screen = t.screen()
    t.capture("at-beta-after-switch")
    ctx.check("at.beta_files_offered_same_logical_path", "harbor_tide" in beta_screen, beta_screen[-400:])
    ctx.check("at.no_stale_alpha_results", "lantern_" not in beta_screen)
    t.keys("C-u")


def s08_roles_and_drafts(ctx):
    """Two terminals: watcher drafts, Enter hint, Alt+T transfer, host-enforced read-only."""
    ctx.start_host()
    a = ctx.term("controller")
    a.run(f"{NORI} attach")
    attach_new_session(a)
    a.type("first light")
    a.keys("Enter")
    a.wait_for("heard: first light", 20)
    obs = ctx.observer()
    sid = obs.sessions()[0]["sessionId"]
    b = ctx.term("watcher")
    b.run(f"{NORI} attach {sid[:8]}")
    b.wait_for(r"(WATCHING|watching|Watching)", 20, regex=True)
    b.type("draft from the watcher")
    before_enter = set(b.screen().splitlines())
    b.keys("Enter")
    time.sleep(1)
    screen = b.screen()
    notices = [l for l in screen.splitlines() if l not in before_enter and re.search(r"alt\+t", l, re.I)]
    b.capture("watcher-enter")
    prompts = [r.get("text", "") for r in ctx.ledger("request", "session/prompt")]
    ctx.check("watcher.enter_not_delivered", not any("draft from the watcher" in p for p in prompts))
    ctx.check("watcher.enter_shows_take_control_hint", len(notices) >= 1, notices)
    ctx.check("watcher.enter_notice_shown_once", len(notices) == 1, notices)
    ctx.check("watcher.draft_kept", "draft from the watcher" in screen)
    denied = obs.call("session/set_mode", {"sessionId": sid, "modeId": "approve-all"})
    ctx.check("host.observer_cannot_set_mode", "error" in denied, denied)
    denied = obs.call("session/prompt", {"sessionId": sid, "prompt": [{"type": "text", "text": "sneak"}]})
    ctx.check("host.observer_cannot_prompt", "error" in denied, denied)
    b.keys("M-t")
    b.wait_for(r"(CONTROLLING|controlling|Controlling)", 10, regex=True)
    time.sleep(1)
    a_screen = a.screen()
    a.capture("controller-demoted")
    b.capture("watcher-promoted")
    ctx.check("transfer.old_controller_sees_watching", re.search(r"watching", a_screen, re.I), a_screen[:300])
    ctx.check("transfer.draft_survives_promotion", "draft from the watcher" in b.screen())
    b.keys("C-u")
    b.keys("C-d")
    time.sleep(1.5)
    ctx.check("transfer.no_automatic_promotion", not re.search(r"CONTROLLING", a.screen()), a.screen()[:200])
    obs.close()


def s09_reconnect_keeps_drafts(ctx):
    """A dropped connection shows reconnecting and keeps drafts in both roles."""
    endpoint, _ = ctx.start_host()
    proxy = Proxy(endpoint)
    token_file = ctx.fx["homes"]["host"] / "host" / "token"
    a = ctx.term("controller")
    a.run(f"{NORI} attach --host {proxy.url} --token-file {token_file}")
    attach_new_session(a)
    a.type("half-written controller draft")
    proxy.drop_all(hold=3.0)
    a.wait_for(r"(RECONNECTING|reconnecting|Reconnecting)", 15, regex=True)
    a.capture("reconnecting")
    a.wait_gone("RECONNECTING", 20)
    time.sleep(1)
    ctx.check("reconnect.controller_draft_kept", "half-written controller draft" in a.screen())
    ctx.check("reconnect.controller_role_restored", role_of(a.screen()) == "controlling", role_of(a.screen()))
    a.capture("reconnected")
    proxy.close()


def s10_approvals(ctx):
    """/approvals: host modes for the foreground session only; Ask only where offered; host default on new."""
    ctx.start_host()
    t = ctx.term("attach")
    t.run(f"{NORI} attach")
    attach_new_session(t)
    obs = ctx.observer()
    first = obs.sessions()[0]["sessionId"]
    t.type("/approvals")
    t.keys("Enter")
    time.sleep(1.5)
    menu = t.screen()
    t.capture("approvals-menu")
    ctx.check("approvals.offers_host_modes", all(x in menu for x in ("Ask", "Agent", "Approve all")), menu[-500:])
    ctx.check("approvals.no_legacy_presets", "Full Access" not in menu and "Read Only" not in menu)
    t.keys("Escape")
    ctx.check("approvals.not_sent_as_prompt",
              not any("/approvals" in r.get("text", "") for r in ctx.ledger("request", "session/prompt")))
    obs.close()


def s11_agent_home_regressions(ctx, mode):
    """The six Agent Home features still show in plain nori and in attach."""
    if mode == "attach":
        ctx.start_host()
        t = ctx.term("attach")
        t.run(f"{NORI} attach")
        attach_new_session(t)
    else:
        t = ctx.term("fg")
        t.run(f"{NORI} --skip-trust-directory")
        wait_composer(t)
    t.type("think about the tide")
    t.keys("Enter")
    t.wait_for("heard: think about the tide", 20)
    time.sleep(0.5)
    screen = t.history() if mode == "fg" else t.screen()
    ctx.check("home.thought_text_in_history", "Weighing the lighthouse log" in screen)
    title = t.title()
    ctx.check("home.terminal_title_names_agent", "claude" in title.lower() or "lantern" in title.lower(), title)
    t.type("scan the harbor")
    t.keys("Enter")
    t.wait_for("heard: scan the harbor", 20)
    time.sleep(0.5)
    screen = t.history() if mode == "fg" else t.screen()
    ctx.check("home.read_range_shown", re.search(r"tides\.md.{0,20}(12|L12|:12)", screen) is not None,
              [l for l in screen.splitlines() if "tides" in l])
    ctx.check("home.search_hits_shown", re.search(r"3 matches in 2 files", screen) is not None,
              [l for l in screen.splitlines() if "beacon" in l])
    t.capture("agent-home")


def mode_sets(ctx):
    """(sessionId, adapter mode) the host asked each adapter to enter, in order."""
    return [(r.get("sessionId"), r.get("value")) for r in ctx.ledger("request", "session/set_config_option")
            if r.get("configId") == "mode"]


HOST_MODE_FOR = {"default": "ask", "read-only": "ask", "acceptEdits": "agent", "workspace-write": "agent"}


def choose(t, label, limit=12):
    """Move a list selection onto the row containing `label`, then press Enter."""
    for _ in range(limit):
        row = next((l for l in t.screen().splitlines() if re.match(r"^\s*(›|>|▶|▏)", l) and label in l), None)
        if row:
            t.keys("Enter")
            return True
        t.keys("Down")
    return False


def s12_new_takes_host_default(ctx):
    """[host] default_mode applies to sessions created from the TUI, never the foreground mode."""
    ctx.start_host()
    t = ctx.term("attach")
    t.run(f"{NORI} attach")
    attach_new_session(t)
    time.sleep(1)
    sets = mode_sets(ctx)
    ctx.check("default.first_new_uses_host_default", bool(sets) and HOST_MODE_FOR.get(sets[-1][1]) == "ask", sets)
    t.type("/approvals")
    t.keys("Enter")
    time.sleep(1)
    picked = choose(t, "Approve all")
    time.sleep(1)
    ctx.check("approvals.controller_can_pick_approve_all", picked, last_nonblank(t.screen()))
    t.type("/new")
    t.keys("Enter")
    time.sleep(0.6)
    if re.search(r"^\s*/new\b", t.screen(), re.M):
        t.keys("Enter")
    time.sleep(2)
    sets = mode_sets(ctx)
    ctx.check("default.second_new_does_not_copy_foreground", len({s for s, _ in sets}) >= 2
              and HOST_MODE_FOR.get(sets[-1][1]) == "ask", sets)
    t.capture("after-second-new")


def s13_approvals_per_session(ctx):
    """Mode changes hit only the foreground session; Ask is hidden where the adapter lacks it; watchers cannot change."""
    ctx.start_host()
    a = ctx.term("controller")
    a.run(f"{NORI} attach")
    attach_new_session(a)
    a.type("/new opencode")
    a.keys("Enter")
    time.sleep(0.6)
    if re.search(r"^\s*/new\b", a.screen(), re.M):
        a.keys("Enter")
    time.sleep(2)
    a.type("/approvals")
    a.keys("Enter")
    time.sleep(1.2)
    menu = a.screen()
    a.capture("approvals-opencode")
    ctx.check("approvals.ask_hidden_for_adapter_without_ask",
              "Approve all" in menu and not re.search(r"^\s*(›\s*)?\d?\.?\s*Ask\b", menu, re.M), menu[-400:])
    before = mode_sets(ctx)
    picked = choose(a, "Approve all")
    time.sleep(1)
    after = mode_sets(ctx)
    ctx.check("approvals.change_reaches_only_foreground", picked and len(after) >= len(before), after)
    footer = a.screen()
    ctx.check("approvals.footer_shows_foreground_mode", re.search(r"Approve all", footer.splitlines()[-1] if footer else "")
              or "Approve all" in "\n".join(footer.splitlines()[-4:]), footer.splitlines()[-4:])
    obs = ctx.observer()
    sid = obs.sessions()[0]["sessionId"]
    obs.close()
    b = ctx.term("watcher")
    b.run(f"{NORI} attach {sid[:8]}")
    b.wait_for(r"(WATCHING|watching|Watching)", 20, regex=True)
    b.type("/approvals")
    time.sleep(1)
    row = next((l for l in b.screen().splitlines() if l.strip().startswith("/approvals")), "")
    b.capture("watcher-approvals")
    ctx.check("approvals.watcher_sees_greyed_with_take_control", re.search(r"alt\+t", row, re.I), row)
    b.keys("C-u")


def s14_picker_scope_and_resume(ctx):
    """The picker lists only the connected host; a chosen agent server's saved sessions resume there."""
    ctx.start_host()
    laptop_env = ctx.env("laptop")
    subprocess.run([str(BIN / "nori"), "host", "--port", "0"], env=laptop_env, cwd=ctx.fx["roots"]["laptop"],
                   capture_output=True, timeout=60)
    subprocess.run([str(BIN / "nori"), "host", "new", "--prompt", "laptop-only errand"], env=laptop_env,
                   cwd=ctx.fx["roots"]["laptop"], capture_output=True, timeout=60)
    try:
        endpoint, _ = wire.host_endpoint(ctx.fx["homes"]["host"])
        token_file = ctx.fx["homes"]["host"] / "host" / "token"
        t = ctx.term("attach", cwd="laptop", home="laptop")
        t.run(f"{NORI} attach --host {endpoint} --token-file {token_file}")
        attach_new_session(t)
        t.type("/sessions")
        t.keys("Enter")
        time.sleep(1.5)
        listing = t.screen()
        t.capture("picker")
        ctx.check("picker.excludes_other_hosts", "laptop-only errand" not in listing)
        t.keys("Escape")
        t.type("/resume")
        t.keys("Enter")
        time.sleep(2)
        t.capture("resume-picker")
        found = "Saved orchard survey" in t.screen()
        ctx.check("resume.lists_agent_server_saved_sessions", found, t.screen()[-400:])
        if found:
            choose(t, "Saved orchard survey")
            time.sleep(2)
        resumed = [r for r in ctx.ledger("request") if r.get("method") in ("session/load", "session/resume")
                   and r.get("sessionId") == "saved-orchard"]
        ctx.check("resume.adapter_loaded_saved_session_on_host", bool(resumed), resumed)
    finally:
        subprocess.run([str(BIN / "nori"), "host", "stop"], env=laptop_env, capture_output=True)


def s15_legacy_approval_migration(ctx, preset):
    """Legacy presets migrate once: Full Access -> Approve all (sandbox kept), Read Only -> Ask or Agent."""
    legacy = {"full-access": 'approval_policy = "never"\nsandbox_mode = "danger-full-access"',
              "read-only": 'approval_policy = "on-request"\nsandbox_mode = "read-only"',
              "read-only-no-ask": 'approval_policy = "on-request"\nsandbox_mode = "read-only"'}[preset]
    agent = "opencode" if preset == "read-only-no-ask" else "claude-code"
    ctx.fx = fixtures.build(ctx.run_dir, default_agent=agent, top_extra=legacy)
    config = ctx.fx["homes"]["host"] / "config.toml"
    original = config.read_text()
    t = ctx.term("fg")
    t.run(f"{NORI} --skip-trust-directory")
    wait_composer(t)
    time.sleep(1.5)
    sets = mode_sets(ctx)
    want = {"full-access": "approve-all", "read-only": "ask", "read-only-no-ask": "agent"}[preset]
    t.type("/approvals")
    t.keys("Enter")
    time.sleep(1.2)
    menu = t.screen()
    t.capture("approvals-after-migration")
    label = {"approve-all": "Approve all", "ask": "Ask", "agent": "Agent"}[want]
    current = [l for l in menu.splitlines() if re.search(r"(current|✓|\(selected\))", l, re.I)]
    ctx.check(f"migration.{preset}_maps_to_{want}", any(label in l for l in current), current or menu[-400:])
    t.keys("Escape")
    t.type("/quit")
    t.keys("Enter")
    time.sleep(1.5)
    migrated = config.read_text()
    sandbox_line = re.search(r"^sandbox_mode\s*=.*$", original, re.M).group(0)
    ctx.check("migration.sandbox_setting_unchanged", sandbox_line in migrated, sandbox_line)
    (ctx.dir / "config-before.toml").write_text(original)
    (ctx.dir / "config-after.toml").write_text(migrated)
    t.run(f"{NORI} --skip-trust-directory")
    wait_composer(t)
    time.sleep(1)
    t.type("/quit")
    t.keys("Enter")
    time.sleep(1.5)
    ctx.check("migration.runs_once", config.read_text() == migrated)


def attach_via(ctx, t, proxy, sid=None):
    token_file = ctx.fx["homes"]["host"] / "host" / "token"
    t.run(f"{NORI} attach --host {proxy.url} --token-file {token_file}" + (f" {sid[:8]}" if sid else ""))


def role_of(screen):
    """The role in the attach header row (`Nori host  CONTROLLING` / `WATCHING ...`)."""
    header = next((l for l in screen.splitlines() if "Nori host" in l), "")
    for role in ("CONTROLLING", "WATCHING", "RECONNECTING"):
        if role in header:
            return role.lower()
    return f"unknown: {header.strip()!r}"


def s16_watcher_reconnect_keeps_draft(ctx):
    """A watcher whose link drops keeps its unsent draft, stays a watcher, and still cannot send."""
    endpoint, _ = ctx.start_host()
    a = ctx.term("controller")
    a.run(f"{NORI} attach")
    attach_new_session(a)
    a.type("first light")
    a.keys("Enter")
    a.wait_for("heard: first light", 20)
    obs = ctx.observer()
    sid = obs.sessions()[0]["sessionId"]
    obs.close()
    proxy = Proxy(endpoint)
    b = ctx.term("watcher")
    attach_via(ctx, b, proxy, sid)
    b.wait_for(r"(WATCHING|watching|Watching|take control)", 20, regex=True)
    draft = "watcher draft across the gap"
    b.type(draft)
    proxy.drop_all(hold=3.0)
    try:
        b.wait_for(r"(RECONNECTING|reconnecting|Reconnecting)", 15, regex=True)
        saw = True
    except TimeoutError:
        saw = False
    b.capture("watcher-reconnecting")
    ctx.check("watcher_reconnect.shows_reconnecting", saw)
    ctx.check("watcher_reconnect.draft_kept_while_down", draft in b.screen())
    b.wait_gone("RECONNECTING", 25)
    time.sleep(1.5)
    b_screen = b.screen()
    b.capture("watcher-reconnected")
    ctx.check("watcher_reconnect.draft_kept_after", draft in b_screen)
    ctx.check("watcher_reconnect.still_watching", role_of(b_screen) == "watching", role_of(b_screen))
    ctx.check("watcher_reconnect.controller_unchanged", role_of(a.screen()) == "controlling", role_of(a.screen()))
    a.type("second light")
    a.keys("Enter")
    try:
        b.wait_for("heard: second light", 15)
        live = True
    except TimeoutError:
        live = False
    ctx.check("watcher_reconnect.sees_new_turns", live)
    b.keys("Enter")
    time.sleep(1.2)
    prompts = [r.get("text", "") for r in ctx.ledger("request", "session/prompt")]
    ctx.check("watcher_reconnect.enter_still_not_delivered", not any(draft in p for p in prompts), prompts)
    ctx.check("watcher_reconnect.draft_kept_after_enter", draft in b.screen())
    b.capture("watcher-enter-after-reconnect")
    b.keys("C-u")
    proxy.close()


def s17_silent_loss(ctx):
    """A blackholed link (no FIN/RST) is noticed, reconnects, keeps the draft and the live turn."""
    endpoint, _ = ctx.start_host()
    proxy = Proxy(endpoint)
    a = ctx.term("controller")
    attach_via(ctx, a, proxy)
    attach_new_session(a)
    a.type("hold the lamp")
    a.keys("Enter")
    a.wait_for("tick 3", 20)
    draft = "draft through the silence"
    a.type(draft)
    t0 = time.time()
    n = proxy.silence_all()
    try:
        a.wait_for(r"(RECONNECTING|reconnecting|Reconnecting|stopped responding)", 30, regex=True)
        noticed = time.time() - t0
    except TimeoutError:
        noticed = None
    a.capture("silent-noticed")
    ctx.check("silent.client_notices_without_fin", noticed is not None and noticed <= 20,
              f"links silenced={n} noticed_after={noticed and round(noticed, 1)}s")
    ctx.check("silent.draft_kept_while_down", draft in a.screen())
    a.wait_gone("RECONNECTING", 30)
    time.sleep(2)
    screen = a.screen()
    a.capture("silent-reconnected")
    ctx.check("silent.draft_kept_after", draft in screen)
    before = ctx.ticks()
    time.sleep(1.5)
    ctx.check("silent.turn_kept_running", ctx.ticks() > before, f"{before}->{ctx.ticks()}")
    n1 = max(map(int, re.findall(r"tick (\d+)", a.screen())) or [0])
    time.sleep(1.2)
    n2 = max(map(int, re.findall(r"tick (\d+)", a.screen())) or [0])
    ctx.check("silent.live_stream_resumes_on_screen", n2 > n1 > 0, f"{n1}->{n2}")
    role = role_of(screen)
    ctx.check("silent.role_after_reconnect_recorded", True, role)
    if role == "watching":
        a.keys("M-t")
        time.sleep(1.5)
    ctx.check("silent.can_control_after_reconnect", role_of(a.screen()) == "controlling", role_of(a.screen()))
    a.keys("C-u")
    a.keys("C-c")
    time.sleep(2)
    cancels = ctx.ledger("request", "session/cancel")
    ends = [r for r in ctx.ledger("turn_end") if r.get("stopReason") == "cancelled"]
    ctx.check("silent.cancel_after_reconnect_reaches_adapter", bool(cancels) and bool(ends),
              {"cancels": len(cancels), "ends": ends[-1:]})
    a.capture("silent-after-cancel")
    proxy.close()


def indicator_lines(screen, needle):
    return [l for l in screen.splitlines() if needle in l]


def s18_thought_indicator(ctx, mode):
    """The working indicator follows the agent's current thought, heading or plain text."""
    if mode == "attach":
        ctx.start_host()
        t = ctx.term("attach")
        t.run(f"{NORI} attach")
        attach_new_session(t)
    else:
        t = ctx.term("fg")
        t.run(f"{NORI} --skip-trust-directory")
        wait_composer(t)
    t.type("ponder the harbor")
    t.keys("Enter")
    expected = {1: "Charting the breakwater", 2: "Sorting the buoy colours", 3: "Listening for the foghorn cadence"}
    seen = {}
    for n, text in expected.items():
        deadline = time.time() + 15
        while time.time() < deadline and not [r for r in ctx.ledger("thought") if r.get("n") == n]:
            time.sleep(0.1)
        time.sleep(0.8)
        screen = t.screen()
        seen[n] = screen
        t.capture(f"indicator-thought-{n}")
        rows = indicator_lines(screen, text)
        working = [l for l in screen.splitlines() if re.search(r"(esc to interrupt|\(\d+s|Working)", l)]
        ctx.check(f"indicator.thought_{n}_shown", bool(rows), {"rows": rows, "working": working})
        ctx.check(f"indicator.thought_{n}_on_working_row", any(r in working for r in rows), {"rows": rows, "working": working})
    ctx.check("indicator.second_heading_replaces_first",
              not [l for l in indicator_lines(seen[2], expected[1]) if re.search(r"(esc to interrupt|\(\d+s)", l)])
    ctx.check("indicator.plain_thought_replaces_heading",
              not [l for l in indicator_lines(seen[3], expected[2]) if re.search(r"(esc to interrupt|\(\d+s)", l)])
    t.wait_for("heard: ponder the harbor", 20)
    time.sleep(0.8)
    done = t.history() if mode == "fg" else t.screen()
    t.capture("indicator-done")
    ctx.check("indicator.cleared_after_turn",
              not [l for l in t.screen().splitlines() if "foghorn" in l and re.search(r"(esc to interrupt|\(\d+s)", l)])
    ctx.check("indicator.thoughts_in_history", all(x in done for x in expected.values()),
              [x for x in expected.values() if x not in done])


def s19_tool_output_cap(ctx, mode, capped):
    """[tui] tool_output_max_lines caps tool text head/tail; diffs stay whole; default stays full."""
    if mode == "attach":
        ctx.start_host()
        t = ctx.term("attach", rows=70)
        t.run(f"{NORI} attach")
        attach_new_session(t)
    else:
        t = ctx.term("fg", rows=70)
        t.run(f"{NORI} --skip-trust-directory")
        wait_composer(t)
    t.type("bulky rows")
    t.keys("Enter")
    t.wait_for("heard: bulky rows", 20)
    time.sleep(0.8)
    text = t.history() if mode == "fg" else t.screen()
    t.capture("bulky")
    (ctx.dir / "bulky-history.txt").write_text(text)
    shown = sorted({int(m) for m in re.findall(r"row (\d\d) of the synthetic ledger", text)})
    hidden = re.findall(r"(\d+) lines? hidden", text)
    if capped:
        ctx.check("cap.head_and_tail_kept", shown[:1] == [1] and shown[-1:] == [60], shown)
        ctx.check("cap.total_rows_at_most_cap", 0 < len(shown) <= 10, f"{len(shown)} rows: {shown}")
        ctx.check("cap.hidden_marker_counts_the_rest", hidden == [str(60 - len(shown))], hidden)
    else:
        ctx.check("cap.default_shows_all_rows", shown == list(range(1, 61)), f"{len(shown)} rows")
        ctx.check("cap.default_has_no_hidden_marker", not hidden, hidden)
    t.type("patch the buoys")
    t.keys("Enter")
    t.wait_for("heard: patch the buoys", 20)
    time.sleep(0.8)
    text = t.history() if mode == "fg" else t.screen()
    t.capture("patch")
    greens = sorted({int(m) for m in re.findall(r"buoy (\d\d) = green", text)})
    ctx.check("cap.diff_left_whole", greens == list(range(1, 41)), f"{len(greens)} added lines")
    if capped and mode == "fg":
        t.keys("C-t")
        time.sleep(1.2)
        t.capture("transcript")
        tr = t.screen()
        t.keys("q")
        rows = sorted({int(m) for m in re.findall(r"row (\d\d) of the synthetic ledger", tr)})
        ctx.check("cap.transcript_overlay_is_uncapped", rows[:12] == list(range(1, 13))
                  and not re.search(r"\d+ lines hidden", tr), f"contiguous rows on first page: {rows[:14]}")


def s20_agent_started_cancel(ctx, mode):
    """A controller interrupts a turn the agent started; the adapter sees one cancel; the host reports it."""
    if mode == "attach":
        ctx.start_host()
        t = ctx.term("attach")
        t.run(f"{NORI} attach")
        attach_new_session(t)
    else:
        t = ctx.term("fg")
        t.run(f"{NORI} --skip-trust-directory")
        wait_composer(t)
    t.type("wake the keeper")
    t.keys("Enter")
    t.wait_for("heard: wake the keeper", 20)
    deadline = time.time() + 15
    while time.time() < deadline and len([r for r in ctx.ledger("tick") if r.get("turnId")]) < 4:
        time.sleep(0.2)
    started = ctx.ledger("agent_turn_start")
    ctx.check("agent_turn.started_without_prompt", bool(started)
              and len(ctx.ledger("request", "session/prompt")) == 1, started)
    t.capture("agent-turn-working")
    working = t.screen()
    ctx.check("agent_turn.tui_shows_working", re.search(r"(esc to interrupt|Working|working|\(\d+s)", working) is not None,
              last_nonblank(working))
    b = None
    if mode == "attach":
        obs = ctx.observer()
        sid = obs.sessions()[0]["sessionId"]
        obs.close()
        b = ctx.term("watcher")
        b.run(f"{NORI} attach {sid[:8]}")
        b.wait_for(r"(WATCHING|watching|take control)", 20, regex=True)
        b.keys("C-c")
        time.sleep(1.2)
        ctx.check("agent_turn.watcher_cannot_cancel", not ctx.ledger("request", "session/cancel"),
                  len(ctx.ledger("request", "session/cancel")))
        b.capture("watcher-ctrl-c")
    draft = "note for after the keeper"
    if mode == "attach":
        t.type(draft)
        t.keys("C-c")
    else:
        t.keys("Escape")
    deadline = time.time() + 10
    while time.time() < deadline and not ctx.ledger("agent_turn_end"):
        time.sleep(0.2)
    time.sleep(1.5)
    cancels = ctx.ledger("request", "session/cancel")
    end = ctx.ledger("agent_turn_end")
    ctx.check("agent_turn.adapter_received_one_cancel", len(cancels) == 1, len(cancels))
    ctx.check("agent_turn.adapter_reports_cancelled_same_turn", bool(end) and end[-1]["stopReason"] == "cancelled"
              and started and end[-1]["turnId"] == started[-1]["turnId"], end)
    before = ctx.ticks()
    time.sleep(1.0)
    ctx.check("agent_turn.ticks_stopped", ctx.ticks() == before)
    screen = t.screen()
    t.capture("agent-turn-cancelled")
    ctx.check("agent_turn.tui_back_to_idle", not re.search(r"(esc to interrupt|Cancel requested)", screen),
              [l for l in screen.splitlines() if re.search(r"(interrupt|Cancel|cancel|idle|ready)", l)])
    ctx.check("agent_turn.no_unconfirmed_warning", not re.search(r"unconfirmed", screen, re.I))
    if mode == "attach":
        ctx.check("agent_turn.draft_kept", draft in screen)
        time.sleep(0.5)
        b.capture("watcher-after-cancel")
        ctx.check("agent_turn.watcher_sees_idle", not re.search(r"(esc to interrupt|Cancel requested)", b.screen()))
        t.keys("C-c")
        time.sleep(0.6)
        ctx.check("agent_turn.info_second_ctrl_c_when_idle", True,
                  "draft kept" if draft in t.screen() else "draft cleared (idle Ctrl+C clears the composer)")
        t.capture("second-ctrl-c-idle")
    t.keys("C-u")
    t.type("after the keeper")
    t.keys("Enter")
    try:
        t.wait_for("heard: after the keeper", 15)
        ok = True
    except TimeoutError:
        ok = False
    ctx.check("agent_turn.next_prompt_works", ok)


def child_pids(pid):
    out = subprocess.run(["pgrep", "-P", str(pid)], capture_output=True, text=True).stdout.split()
    return [int(p) for p in out]


def descendants(pid):
    found, todo = [], [int(pid)]
    while todo:
        kids = child_pids(todo.pop())
        found += kids
        todo += kids
    return found


def nori_pid(t):
    kids = child_pids(t.fmt("#{pane_pid}"))
    return next((k for k in kids if "nori" in Path(f"/proc/{k}/cmdline").read_bytes().decode(errors="replace")), None)


def listening_ports(pid):
    """TCP ports a process listens on, from /proc (no ss needed)."""
    inodes = set()
    for fd in Path(f"/proc/{pid}/fd").iterdir():
        try:
            target = fd.readlink().as_posix()
        except OSError:
            continue
        if target.startswith("socket:["):
            inodes.add(target[8:-1])
    ports = []
    for table in ("tcp", "tcp6"):
        for line in Path(f"/proc/net/{table}").read_text().splitlines()[1:]:
            cols = line.split()
            if cols[3] == "0A" and cols[9] in inodes:
                ports.append(int(cols[1].split(":")[1], 16))
    return ports


def s21_foreground_owns_host(ctx, how):
    """Plain nori runs a private host: no listener or discovery files; every exit path ends every session."""
    t = ctx.term("fg")
    t.run(f"{NORI} --skip-trust-directory")
    wait_composer(t)
    pid = nori_pid(t)
    host_dir = ctx.fx["homes"]["host"] / "host"
    ctx.check("owned.no_discovery_files", not (host_dir / "state.json").exists() and not (host_dir / "token").exists(),
              sorted(p.name for p in host_dir.iterdir()) if host_dir.exists() else "no host dir")
    ctx.check("owned.no_listening_socket", pid and not listening_ports(pid), pid and listening_ports(pid))
    ls = ctx.nori("host", "ls")
    ctx.check("owned.not_visible_to_host_ls", "claude" not in ls.stdout, (ls.stdout + ls.stderr)[-200:])
    t.type("hold the lamp")
    t.keys("Enter")
    wait_ticks(ctx, 3)
    t.type("/new")
    time.sleep(0.4)
    adapters = sorted({r["pid"] for r in ctx.ledger("spawn")})
    agent_tree = descendants(pid)
    ctx.check("owned.adapters_are_children_of_nori", adapters and all(a in agent_tree for a in adapters),
              {"adapters": adapters, "tree": agent_tree})
    t.keys("C-u")
    t.capture("owned-working")
    if how == "hangup":
        t.close()
    elif how == "sigterm":
        subprocess.run(["kill", "-TERM", str(pid)])
    deadline = time.time() + 10
    while time.time() < deadline and running(pid):
        time.sleep(0.2)
    ctx.check("owned.nori_exits", not running(pid), how)
    if how == "sigterm":
        deadline = time.time() + 5
        while time.time() < deadline and not t.at_shell():
            time.sleep(0.2)
        t.capture("after-sigterm")
        ctx.check("owned.terminal_restored_after_sigterm", t.at_shell() and not t.alt_screen() and t.cursor_visible(),
                  last_nonblank(t.screen()))
    time.sleep(1.0)
    alive = [a for a in adapters if running(a)]
    ctx.check("owned.no_adapter_outlives_nori", not alive, alive)
    before = ctx.ticks()
    time.sleep(1.2)
    ctx.check("owned.turn_stopped", ctx.ticks() == before, f"{before}->{ctx.ticks()}")


def s21c_private_beside_background_host(ctx):
    """A background host in the same NORI_HOME and plain nori never see each other's sessions."""
    ctx.start_host()
    obs = ctx.observer()
    t = ctx.term("fg")
    t.run(f"{NORI} --skip-trust-directory")
    wait_composer(t)
    t.type("hello lantern")
    t.keys("Enter")
    t.wait_for("heard: hello lantern", 20)
    live = [s for s in obs.sessions() if (s.get("_meta") or {}).get("nori/session", {}).get("live")]
    ctx.check("owned.background_host_lists_no_live_foreground_session", live == [], obs.sessions())
    t.type("/quit")
    t.keys("Enter")
    time.sleep(1.5)
    if not t.at_shell():
        t.keys("Enter")
    ls = ctx.nori("host", "ls")
    ctx.check("owned.background_host_survives_foreground_quit", ls.returncode == 0, (ls.stdout + ls.stderr)[-200:])
    obs.close()


def s22_switch_agents(ctx):
    """/agent switches the foreground session to another agent server on the owned host."""
    t = ctx.term("fg")
    t.run(f"{NORI} --skip-trust-directory")
    wait_composer(t)
    t.type("hello lantern")
    t.keys("Enter")
    t.wait_for("claude-code heard: hello lantern", 20)
    t.type("/agent")
    t.keys("Enter")
    time.sleep(1.5)
    t.capture("agent-picker")
    picker = t.screen()
    ctx.check("agent.picker_lists_configured_servers", all(n in picker for n in ("Lantern Claude", "Lantern Codex")),
              [l for l in picker.splitlines() if "Lantern" in l])
    picked = choose(t, "Lantern Codex")
    time.sleep(2.0)
    if "Start a new session" in t.screen():
        t.capture("agent-session-choice")
        choose(t, "Start a new session")
    time.sleep(2.5)
    t.capture("after-switch")
    codex_new = [r for r in ctx.ledger("request", "session/new") if r["adapter"] == "codex"]
    ctx.check("agent.switch_starts_codex_session", picked and bool(codex_new), codex_new)
    ctx.check("agent.switch_keeps_folder", bool(codex_new) and codex_new[-1].get("cwd") == str(ctx.fx["roots"]["alpha"]),
              codex_new[-1:])
    t.type("hello again")
    t.keys("Enter")
    try:
        t.wait_for("codex heard: hello again", 20)
        ok = True
    except TimeoutError:
        ok = False
    ctx.check("agent.prompt_reaches_new_server", ok, last_nonblank(t.screen()))
    claude_after = [r for r in ctx.ledger("request", "session/prompt") if r["adapter"] == "claude-code"]
    ctx.check("agent.old_server_gets_no_new_prompt", len(claude_after) == 1, len(claude_after))
    t.capture("codex-reply")
    pid = nori_pid(t)
    t.type("/quit")
    t.keys("Enter")
    time.sleep(1.5)
    if not t.at_shell():
        t.keys("Enter")
    time.sleep(1)
    adapters = sorted({r["pid"] for r in ctx.ledger("spawn")})
    ctx.check("agent.all_servers_stopped_on_quit", not [a for a in adapters if running(a)], adapters)


LOCAL_STEPS = [
    ("/status", "Escape", r"Session ID"),
    ("/diff", "q", r"lantern_wick\.rs"),
    ("/first-prompt", None, r"hello lantern"),
    ("/memory", None, r"(Lighthouse rule 7|instruction files)"),
    ("/context", "Escape", r"(?i)context"),
    ("/model", "Escape", r"(?i)(model|Mode)"),
    ("/config", "Escape", r"Mode"),
    ("/approvals", "Escape", r"Agent"),
    ("!echo shell-local-ok", None, r"shell-local-ok"),
    ("/fork", "Escape", r"hello lantern"),
    ("/sessions", "Escape", r"(?i)session"),
    ("/resume", "Escape", r"Saved orchard survey"),
]
VOLATILE = [(re.compile(r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}"), "<uuid>"),
            (re.compile(r"\(\d+s"), "(Ns"), (re.compile(r"\b\d+(\.\d+)?\s?(ms|s)\b"), "<dur>"),
            (re.compile(r"\b\d{1,2}:\d{2}(:\d{2})?\s?(AM|PM|am|pm)?\b"), "<time>"),
            (re.compile(r"(just now|\d+ (seconds?|minutes?) ago)"), "<ago>"),
            (re.compile(r"/tmp/nori-host-tui-acceptance(-\w+)?/[A-Za-z0-9]+/"), "<run>/"),
            (re.compile(r"index [0-9a-f]+\.\.[0-9a-f]+"), "index <h>..<h>")]


def normalize(text):
    for pattern, repl in VOLATILE:
        text = pattern.sub(repl, text)
    return "\n".join(l.rstrip() for l in text.splitlines() if l.strip() and not HINT_TOKENS.search(l))


def s23_local_commands(ctx):
    """Client-local commands keep working in plain nori and never reach the agent as prompts."""
    root = ctx.fx["roots"]["alpha"]
    (root / "AGENTS.md").write_text("Lighthouse rule 7: keep the lamp trimmed.\n")
    subprocess.run("git init -q && git add -A && git -c user.name=Synthetic -c user.email=synthetic@example.invalid "
                   "-c commit.gpgsign=false commit -qm seed && echo changed >> src/lantern_wick.rs",
                   shell=True, cwd=root, check=True,
                   env={"PATH": "/usr/bin:/bin", "HOME": str(ctx.fx["homes"]["host"]), "GIT_CONFIG_NOSYSTEM": "1"})
    t = ctx.term("fg", rows=40)
    t.run(f"{NORI} --skip-trust-directory")
    wait_composer(t)
    t.type("hello lantern")
    t.keys("Enter")
    t.wait_for("heard: hello lantern", 20)
    time.sleep(0.5)
    norm = {}
    for i, (cmd, leave, expect) in enumerate(LOCAL_STEPS):
        t.type(cmd)
        time.sleep(0.5)
        t.keys("Enter")
        time.sleep(0.5)
        if cmd.startswith("/") and re.search(rf"^\s*{re.escape(cmd)}\s*$", t.screen(), re.M):
            t.keys("Enter")
        time.sleep(1.5)
        label = f"local-{i:02d}-{re.sub(r'[^a-z]+', '-', cmd.lower()).strip('-')}"
        t.capture(label)
        screen = t.screen()
        norm[cmd] = normalize(screen)
        (ctx.dir / f"{label}.norm.txt").write_text(norm[cmd])
        ctx.check(f"local.{cmd.split()[0].lstrip('/!') or 'shell'}_works", re.search(expect, screen) is not None,
                  last_nonblank(screen))
        if leave:
            t.keys(leave)
            time.sleep(0.8)
        t.keys("C-u")
    prompts = [r.get("text", "") for r in ctx.ledger("request", "session/prompt")]
    leaked = [c for c, _, _ in LOCAL_STEPS if any(p.strip().splitlines()[-1].startswith(c) for p in prompts if p.strip())]
    ctx.check("local.none_sent_to_agent_as_prompt", len(prompts) == 1 and not leaked, {"prompts": len(prompts), "leaked": leaked})
    news = ctx.ledger("request", "session/new")
    ctx.check("local.no_extra_sessions_started", len(news) == 1, [(r["adapter"], r.get("cwd")) for r in news])
    (ctx.dir / "local-normalized.json").write_text(json.dumps(norm, indent=1))


def slash(t, cmd, settle=1.5):
    """Type a slash command, pause so Enter is not taken as paste, submit it."""
    t.type(cmd)
    time.sleep(1.0)
    t.keys("Enter")
    time.sleep(0.6)
    if re.search(rf"^\s*›?\s*{re.escape(cmd)}\s*$", t.screen(), re.M):
        t.keys("Enter")
    time.sleep(settle)


def submit(t, text):
    t.type(text)
    time.sleep(1.0)
    t.keys("Enter")


def start_fg(ctx, name="fg", cmd=None, rows=36):
    t = ctx.term(name, rows=rows)
    t.run(cmd or f"{NORI} --skip-trust-directory")
    wait_composer(t)
    return t


def start_attach(ctx, name, cwd="alpha", sid=None):
    t = ctx.term(name, cwd=cwd)
    t.run(f"{NORI} attach" + (f" {sid}" if sid else ""))
    if sid:
        t.wait_for("›", 20)
    else:
        attach_new_session(t)
    return t


def wait_ledger(ctx, pred, timeout=15):
    deadline = time.time() + timeout
    while time.time() < deadline:
        rows = [r for r in ctx.ledger() if pred(r)]
        if rows:
            return rows
        time.sleep(0.2)
    return []


def quit_fg(t):
    slash(t, "/quit", settle=1.0)
    deadline = time.time() + 8
    while time.time() < deadline and not t.at_shell():
        time.sleep(0.3)


def perm_responses(ctx):
    return [r for r in ctx.ledger("response") if isinstance(r.get("id"), int) and r["id"] >= 9000]


def s24_picker_first_resume(ctx):
    """`nori resume` shows its picker before any session; Esc starts a fresh session on the owned host."""
    first = ctx.term("first", cwd="beta")
    first.run(f"{NORI} --skip-trust-directory")
    wait_composer(first)
    submit(first, "chart the orchard")
    first.wait_for("heard: chart the orchard", 20)
    quit_fg(first)
    mark = len(ctx.ledger())
    later = lambda: ctx.ledger()[mark:]
    t = ctx.term("fg", cwd="beta")
    t.run(f"{NORI} resume --skip-trust-directory")
    t.wait_for("Resume a previous session", 20)
    time.sleep(1.5)
    t.capture("resume-first")
    # Information: rows listed for the earlier session (none at main for this adapter either).
    (ctx.dir / "resume-rows.txt").write_text("\n".join(l for l in t.screen().splitlines() if l.strip()))
    ctx.check("picker_first.picker_before_any_session",
              not [r for r in later() if r.get("method") in ("session/new", "session/load", "session/resume")],
              [r.get("method") or r["kind"] for r in later()])
    pid = nori_pid(t)
    ctx.check("picker_first.no_listening_socket", pid and not listening_ports(pid), pid and listening_ports(pid))
    t.keys("Escape")
    wait_composer(t)
    time.sleep(1.0)
    submit(t, "after the orchard")
    try:
        t.wait_for("heard: after the orchard", 20)
        ok = True
    except TimeoutError:
        ok = False
    t.capture("fresh-reply")
    news = [r for r in later() if r.get("method") == "session/new"]
    ctx.check("picker_first.esc_starts_one_session_in_launch_folder",
              ok and len(news) == 1 and news[0].get("cwd") == str(ctx.fx["roots"]["beta"]), news)
    pids = {r["pid"] for r in ctx.ledger("spawn")}
    quit_fg(t)
    time.sleep(1)
    ctx.check("picker_first.adapters_stopped_on_quit", not [p for p in pids if running(p)], sorted(pids))


def s25_permission_attribution(ctx, mode):
    """A permission request is answered once by the terminal that sees it; a watcher cannot answer."""
    if mode == "fg":
        t = start_fg(ctx)
        submit(t, "perm the boathouse")
        t.wait_for("Paint the boathouse", 20)
        time.sleep(1.0)
        t.capture("perm-overlay")
        choose(t, "Allow once")
        try:
            t.wait_for("permission:", 15)
            ok = True
        except TimeoutError:
            ok = False
        t.capture("perm-answered")
        answers = perm_responses(ctx)
        ctx.check("perm.adapter_gets_selected_option", ok and answers
                  and (answers[0].get("body") or {}).get("outcome", {}).get("optionId") == "allow",
                  [a.get("body") for a in answers])
        time.sleep(1.5)
        ctx.check("perm.answered_exactly_once", len(perm_responses(ctx)) == 1, len(perm_responses(ctx)))
        submit(t, "perm again")
        t.wait_for(r"Paint the boathouse", 20)
        time.sleep(1.0)
        choose(t, "Reject")
        time.sleep(2.0)
        t.capture("perm-second")
        answers = perm_responses(ctx)
        ctx.check("perm.second_request_answered_separately",
                  len(answers) == 2 and (answers[1].get("body") or {}).get("outcome", {}).get("optionId") == "reject",
                  [a.get("body") for a in answers])
        quit_fg(t)
        return
    ctx.start_host()
    a = start_attach(ctx, "controller")
    sid = ctx.observer().sessions()[0]["sessionId"]
    w = start_attach(ctx, "watcher", sid=sid)
    time.sleep(1.5)
    submit(a, "perm the boathouse")
    a.wait_for("Paint the boathouse", 20)
    time.sleep(1.5)
    w.capture("watcher-sees-request")
    w.keys("Enter")
    time.sleep(1.5)
    ctx.check("perm.watcher_enter_does_not_answer", not perm_responses(ctx), [a.get("body") for a in perm_responses(ctx)])
    a.capture("controller-overlay")
    choose(a, "Allow once")
    time.sleep(2.5)
    answers = perm_responses(ctx)
    ctx.check("perm.controller_answer_reaches_adapter_once", len(answers) == 1
              and (answers[0].get("body") or {}).get("outcome", {}).get("optionId") == "allow",
              [x.get("body") for x in answers])
    w.capture("watcher-after-answer")
    ctx.check("perm.watcher_sees_outcome", "permission:" in w.screen() and "Paint the boathouse" not in
              "\n".join(w.screen().splitlines()[-8:]), last_nonblank(w.screen()))


def s26_queued_followup(ctx, how):
    """A follow-up typed during a turn is delivered exactly once, reclaimed and never sent, or resumed once."""
    t = start_fg(ctx)
    submit(t, "ponder the tide" if how == "deliver" else "hold the lamp")
    wait_ledger(ctx, lambda r: r["kind"] in ("thought", "tick"))
    word = {"deliver": "after the tide", "reclaim": "reclaim the lamp", "resume": "resume the lamp"}[how]
    submit(t, word)
    time.sleep(1.5)
    t.capture("queued")
    ctx.check("queue.not_sent_while_turn_runs",
              not [r for r in ctx.ledger("request", "session/prompt") if word in r.get("text", "")])
    sent = lambda: [r for r in ctx.ledger("request", "session/prompt") if word in r.get("text", "")]
    if how == "deliver":
        wait_ledger(ctx, lambda r: r.get("method") == "session/prompt" and word in r.get("text", ""), timeout=20)
        t.wait_for(f"heard: {word}", 15)
        time.sleep(2.0)
        t.capture("delivered")
        ctx.check("queue.delivered_exactly_once", len(sent()) == 1, len(sent()))
        ctx.check("queue.reply_shown", f"heard: {word}" in t.screen(), last_nonblank(t.screen()))
    elif how == "reclaim":
        t.keys("Up")
        time.sleep(1.0)
        t.keys("Enter")
        time.sleep(1.2)
        t.capture("reclaimed")
        composer = [l for l in t.screen().splitlines() if l.lstrip().startswith("›")]
        ctx.check("queue.reclaimed_into_composer", any(word in l for l in composer[-1:]), composer[-2:])
        t.keys("C-u")
        t.keys("Escape")
        time.sleep(3)
        t.capture("after-turn")
        ctx.check("queue.reclaimed_never_sent", not sent(), len(sent()))
    else:
        t.keys("Escape")
        time.sleep(2.0)
        t.capture("paused")
        ctx.check("queue.paused_after_interrupt", re.search(r"(?i)paused", t.screen()) is not None and not sent(),
                  [l for l in t.screen().splitlines() if "queued" in l])
        t.keys("Enter")
        wait_ledger(ctx, lambda r: r.get("method") == "session/prompt" and word in r.get("text", ""), timeout=15)
        time.sleep(3.0)
        t.capture("resumed")
        ctx.check("queue.resume_delivers_exactly_once", len(sent()) == 1, len(sent()))
        ctx.check("queue.resumed_reply_shown", f"heard: {word}" in t.screen(), last_nonblank(t.screen()))
    quit_fg(t)


def s27_standalone_nori_tui(ctx):
    """The standalone nori-tui binary opens the inline TUI on a private host and cleans up on /quit."""
    resolved = Path(ARGS.nori).resolve()
    tui = resolved.parent / resolved.name.replace("nori", "nori-tui", 1)
    if not tui.exists():
        ctx.check("standalone.binary_built", False, f"missing {tui}")
        return
    (BIN / "nori-tui").unlink(missing_ok=True)
    (BIN / "nori-tui").symlink_to(tui)
    t = start_fg(ctx, cmd="nori-tui --skip-trust-directory")
    pid = next((k for k in child_pids(t.fmt("#{pane_pid}"))), None)
    submit(t, "hello from the standalone")
    try:
        t.wait_for("heard: hello from the standalone", 20)
        ok = True
    except TimeoutError:
        ok = False
    t.capture("standalone-reply")
    ctx.check("standalone.reply_shown", ok, last_nonblank(t.screen()))
    ctx.check("standalone.inline", not t.alt_screen())
    ctx.check("standalone.one_session", len(ctx.ledger("request", "session/new")) == 1, len(ctx.ledger("request", "session/new")))
    ctx.check("standalone.no_listening_socket", pid and not listening_ports(pid), pid and listening_ports(pid))
    pids = {r["pid"] for r in ctx.ledger("spawn")}
    quit_fg(t)
    check_restored(ctx, t, "standalone.exit")
    time.sleep(1)
    ctx.check("standalone.adapters_stopped", not [p for p in pids if running(p)], sorted(pids))
    ctx.check("standalone.process_exits", not (pid and running(pid)), pid)


def s28_escape_cancel(ctx, mode):
    """Escape cancels the turn the terminal saw exactly once; the next turn is not cancelled."""
    if mode == "fg":
        t = start_fg(ctx)
    else:
        ctx.start_host()
        t = start_attach(ctx, "attach")
    submit(t, "hold the lamp")
    wait_ticks(ctx, 3)
    t.keys("Escape")
    t.keys("Escape")
    wait_ledger(ctx, lambda r: r["kind"] == "turn_end" and r.get("stopReason") == "cancelled")
    time.sleep(1.0)
    t.capture("after-escape")
    ctx.check("cancel.exactly_one_cancel", len(ctx.ledger("request", "session/cancel")) == 1,
              len(ctx.ledger("request", "session/cancel")))
    submit(t, "fresh after escape")
    try:
        t.wait_for("heard: fresh after escape", 20)
        ok = True
    except TimeoutError:
        ok = False
    time.sleep(1.0)
    t.capture("next-turn")
    ends = [r for r in ctx.ledger("turn_end")]
    ctx.check("cancel.next_turn_completes", ok and ends and ends[-1].get("stopReason") == "end_turn",
              [e.get("stopReason") for e in ends])
    ctx.check("cancel.no_cancel_for_next_turn", len(ctx.ledger("request", "session/cancel")) == 1,
              len(ctx.ledger("request", "session/cancel")))
    if mode == "fg":
        quit_fg(t)


def s29_close_refusal(ctx, mode):
    """A refused /close keeps the session, shows one refusal, and a retry closes it."""
    (ctx.fx["control"] / "close-refuse").write_text("1")
    obs = None
    if mode == "fg":
        t = start_fg(ctx)
    else:
        ctx.start_host()
        t = start_attach(ctx, "attach")
        obs = ctx.observer()
    submit(t, "first light")
    t.wait_for("heard: first light", 20)
    slash(t, "/close", settle=2.5)
    t.capture("refused")
    screen = t.screen() + "\n".join(t.history().splitlines()[-40:]) if mode == "fg" else t.screen()
    shown = len(re.findall(r"close declined by", t.screen()))
    ctx.check("close_refusal.adapter_refused", bool(ctx.ledger("close_refused")))
    ctx.check("close_refusal.shown_once", shown == 1,
              [l.strip() for l in t.screen().splitlines() if "declined" in l])
    if obs:
        live = [s for s in obs.sessions() if (s.get("_meta") or {}).get("nori/session", {}).get("live")]
        ctx.check("close_refusal.session_still_live_on_host", len(live) == 1, obs.sessions())
    submit(t, "still lit")
    try:
        t.wait_for("heard: still lit", 20)
        ok = True
    except TimeoutError:
        ok = False
    ctx.check("close_refusal.session_keeps_working", ok, last_nonblank(t.screen()))
    slash(t, "/close", settle=2.5)
    t.capture("retry-closed")
    closes = ctx.ledger("request", "session/close")
    ctx.check("close_refusal.retry_reaches_adapter", len(closes) == 2, len(closes))
    ctx.check("close_refusal.no_cancel_sent", not ctx.ledger("request", "session/cancel"))
    if obs:
        live = [s for s in obs.sessions() if (s.get("_meta") or {}).get("nori/session", {}).get("live")]
        ctx.check("close_refusal.retry_ends_session_on_host", not live, obs.sessions())
        ctx.check("close_refusal.client_still_attached", t.alt_screen() and not t.at_shell())
        obs.close()
    else:
        ctx.check("close_refusal.retry_leaves_terminal_open", not t.at_shell(), last_nonblank(t.screen()))
        ctx.check("close_refusal.shown_once_after_retry", len(re.findall(r"close declined by", t.screen())) <= 1,
                  [l.strip() for l in t.screen().splitlines() if "declined" in l])
        quit_fg(t)


def s29c_close_refusal_two_clients(ctx):
    """A refused /close from one of two attached terminals reaches the adapter once and shows at most once in each."""
    (ctx.fx["control"] / "close-refuse").write_text("1")
    ctx.start_host()
    a = start_attach(ctx, "controller")
    submit(a, "first light")
    a.wait_for("heard: first light", 20)
    obs = ctx.observer()
    sid = obs.sessions()[0]["sessionId"]
    b = ctx.term("watcher")
    b.run(f"{NORI} attach {sid[:8]}")
    b.wait_for(r"(WATCHING|watching|Watching)", 20, regex=True)
    time.sleep(1.0)
    slash(a, "/close", settle=3.0)
    time.sleep(1.0)
    a.capture("controller-refused")
    b.capture("watcher-refused")
    declined = lambda t: [l.strip() for l in t.screen().splitlines() if "declined" in l]
    ctx.check("close_refusal2.adapter_got_one_close", len(ctx.ledger("request", "session/close")) == 1,
              len(ctx.ledger("request", "session/close")))
    ctx.check("close_refusal2.adapter_refused_once", len(ctx.ledger("close_refused")) == 1, len(ctx.ledger("close_refused")))
    ctx.check("close_refusal2.controller_shows_once", len(re.findall(r"close declined by", a.screen())) == 1, declined(a))
    ctx.check("close_refusal2.watcher_shows_at_most_once", len(re.findall(r"close declined by", b.screen())) <= 1, declined(b))
    live = [s for s in obs.sessions() if (s.get("_meta") or {}).get("nori/session", {}).get("live")]
    ctx.check("close_refusal2.session_still_live_on_host", len(live) == 1, len(live))
    submit(a, "still lit")
    try:
        a.wait_for("heard: still lit", 20)
        b.wait_for("heard: still lit", 10)
        ok = True
    except TimeoutError:
        ok = False
    ctx.check("close_refusal2.session_keeps_working_in_both", ok, (last_nonblank(a.screen()), last_nonblank(b.screen())))
    ctx.check("close_refusal2.counts_unchanged_after_next_turn",
              len(re.findall(r"close declined by", a.screen())) == 1 and len(re.findall(r"close declined by", b.screen())) <= 1,
              (declined(a), declined(b)))
    a.capture("controller-final")
    b.capture("watcher-final")
    obs.close()


def s29d_second_close_while_refusal_pending(ctx):
    """A second /close sent while a refused first is in flight: one adapter close, the refusal shows once, the session keeps working."""
    (ctx.fx["control"] / "close-refuse").write_text("1")
    ctx.start_host()
    t = start_attach(ctx, "attach")
    obs = ctx.observer()
    submit(t, "first light")
    t.wait_for("heard: first light", 20)
    (ctx.fx["control"] / "close-delay").write_text("5")
    slash(t, "/close", settle=0.5)
    wait_ledger(ctx, lambda r: r.get("kind") == "close_delay_start", timeout=10)
    slash(t, "/close", settle=0.5)
    sent_second_at = time.time()
    t.capture("second-close-sent")
    wait_ledger(ctx, lambda r: r.get("kind") == "close_refused", timeout=15)
    time.sleep(4)
    t.capture("after-both")
    closes = ctx.ledger("request", "session/close")
    end = ctx.ledger("close_delay_end")
    ctx.check("close_twice.second_sent_during_first", bool(end) and sent_second_at < end[0]["t"],
              {"second": sent_second_at, "first_reply": end and end[0]["t"]})
    ctx.check("close_twice.adapter_got_one_close", len(closes) == 1, len(closes))
    ctx.check("close_twice.refusal_shown_once", len(re.findall(r"close declined by", t.screen())) == 1,
              [l.strip() for l in t.screen().splitlines() if "close" in l.lower()][-4:])
    live = [s for s in obs.sessions() if (s.get("_meta") or {}).get("nori/session", {}).get("live")]
    ctx.check("close_twice.session_still_live", len(live) == 1, len(live))
    submit(t, "still lit")
    try:
        t.wait_for("heard: still lit", 20)
        ok = True
    except TimeoutError:
        ok = False
    ctx.check("close_twice.session_keeps_working", ok, last_nonblank(t.screen()))
    ctx.check("close_twice.refusal_still_once_after_next_turn", len(re.findall(r"close declined by", t.screen())) == 1,
              [l.strip() for l in t.screen().splitlines() if "declined" in l])
    t.capture("final")
    obs.close()


def s30_prepared_agent(ctx):
    """A host client prepares an agent server, lists its saved sessions, and activates one on that connection."""
    ctx.start_host()
    endpoint, token = wire.host_endpoint(ctx.fx["homes"]["host"])
    c = wire.Wire(endpoint, token, observe=False, log=ctx.dir / "controller-wire.jsonl")
    cap = ((c.init.get("result") or {}).get("agentCapabilities") or {}).get("_meta", {}).get("nori/preparedAgent")
    ctx.check("prepared.capability_advertised", bool(cap and cap.get("agentOption")),
              json.dumps(cap)[:300] if cap else c.init)
    beta = str(ctx.fx["roots"]["beta"])
    prep = c.call("_nori/host/prepare_agent", {"agent": "codex", "cwd": beta})
    choice = prep.get("result") or {}
    ctx.check("prepared.prepare_returns_selection", choice.get("agent") == "codex" and choice.get("cwd") == beta, prep)
    if not choice.get("id"):
        c.close()
        return
    spawns = wait_ledger(ctx, lambda r: r["kind"] == "spawn" and r["adapter"] == "codex")
    listing = c.call("session/list", {"_meta": {"nori/preparedAgent": {"id": choice["id"]}}})
    rows = (listing.get("result") or {}).get("sessions", [])
    ctx.check("prepared.lists_adapter_saved_sessions", any(r.get("sessionId") == "saved-orchard" for r in rows),
              listing)
    ctx.check("prepared.no_host_conversation_before_activation",
              not ctx.observer().sessions(), "observer list")
    act = c.call("session/new", {"cwd": beta, "mcpServers": [],
                                 "_meta": {"nori/preparedAgent": {"id": choice["id"], "savedSessionId": "saved-orchard"}}},
                 timeout=30)
    sid = (act.get("result") or {}).get("sessionId")
    loads = [r for r in ctx.ledger("request") if r.get("method") in ("session/load", "session/resume")
             and r.get("sessionId") == "saved-orchard"]
    codex_spawns = [r for r in ctx.ledger("spawn") if r["adapter"] == "codex"]
    ctx.check("prepared.activation_resumes_on_same_connection",
              bool(sid) and loads and len(codex_spawns) == 1 and loads[0]["pid"] == codex_spawns[0]["pid"],
              {"act": act, "loads": [l.get("method") for l in loads], "spawns": len(codex_spawns)})
    reuse = c.call("session/new", {"cwd": beta, "mcpServers": [],
                                   "_meta": {"nori/preparedAgent": {"id": choice["id"]}}})
    ctx.check("prepared.selection_used_once", "error" in reuse, reuse)
    if sid:
        c.call("session/prompt", {"sessionId": sid, "prompt": [{"type": "text", "text": "orchard check"}]}, timeout=20)
        ctx.check("prepared.prompt_reaches_resumed_session",
                  any(r.get("sessionId") == "saved-orchard" and "orchard check" in r.get("text", "")
                      for r in ctx.ledger("request", "session/prompt")))
    other = c.call("_nori/host/prepare_agent", {"agent": "opencode", "cwd": beta}).get("result") or {}
    oc = wait_ledger(ctx, lambda r: r["kind"] == "spawn" and r["adapter"] == "opencode")
    if other.get("id"):
        c.call("_nori/host/discard_agent", {"id": other["id"]})
    time.sleep(2.0)
    ctx.check("prepared.discard_stops_unused_adapter", oc and not running(oc[0]["pid"]),
              [(r["pid"], running(r["pid"])) for r in oc])
    third = c.call("_nori/host/prepare_agent", {"agent": "opencode", "cwd": beta}).get("result") or {}
    oc2 = wait_ledger(ctx, lambda r: r["kind"] == "spawn" and r["adapter"] == "opencode"
                      and (not oc or r["pid"] != oc[0]["pid"]))
    c.close()
    time.sleep(2.5)
    ctx.check("prepared.disconnect_stops_unused_adapter", third.get("id") and oc2 and not running(oc2[0]["pid"]),
              [(r["pid"], running(r["pid"])) for r in oc2])
    obs = ctx.observer()
    live = [s for s in obs.sessions() if (s.get("_meta") or {}).get("nori/session", {}).get("live")]
    ctx.check("prepared.activated_session_survives_disconnect", any(s["sessionId"] == sid for s in live) if sid else False,
              [s["sessionId"] for s in live])
    obs.close()


def open_resume_rows(t, cmd):
    slash(t, cmd, settle=2.5)
    if cmd == "/agent":
        choose(t, "Lantern Codex")
        time.sleep(2.5)


def s31_restore_through_host(ctx, cmd, refuse=False):
    """/resume or /agent lists the agent server's own saved rows via the owned host and restores the pick there."""
    t = start_fg(ctx)
    submit(t, "hello lantern")
    t.wait_for("heard: hello lantern", 20)
    first = ctx.ledger("request", "session/new")[0]
    open_resume_rows(t, cmd)
    t.capture("rows")
    listed = "Saved orchard survey" in t.screen()
    ctx.check("restore.lists_agent_server_saved_row", listed, [l.strip() for l in t.screen().splitlines() if "│" not in l][-8:])
    prepared = [r for r in ctx.ledger("spawn") if r["pid"] != first["pid"]]
    if refuse:
        (ctx.fx["control"] / "load-refuse").write_text("1")
    if not listed:
        return
    choose(t, "Saved orchard survey")
    time.sleep(3.5)
    t.capture("after-pick")
    loads = [r for r in ctx.ledger("request") if r.get("method") in ("session/load", "session/resume")
             and r.get("sessionId") == "saved-orchard"]
    ctx.check("restore.loaded_by_prepared_server", loads and prepared and loads[0]["pid"] == prepared[0]["pid"],
              {"loads": [(l["pid"], l["method"]) for l in loads], "prepared": [p["pid"] for p in prepared]})
    if refuse:
        ctx.check("restore.refusal_message_shown", "orchard ledger is sealed by" in " ".join(t.screen().split()),
                  [l.strip() for l in t.screen().splitlines() if "sealed" in l or "Failed" in l])
        submit(t, "still here")
        try:
            t.wait_for("heard: still here", 15)
            ok = True
        except TimeoutError:
            ok = False
        last = ctx.ledger("request", "session/prompt")[-1]
        ctx.check("restore.refusal_keeps_current_session", ok and last["sessionId"] == first.get("sessionId", last["sessionId"])
                  and last["pid"] == first["pid"], (last["pid"], first["pid"]))
    else:
        ctx.check("restore.replayed_history_shown", "an earlier synthetic answer" in t.screen(), last_nonblank(t.screen()))
        submit(t, "after the orchard")
        try:
            t.wait_for("heard: after the orchard", 15)
            ok = True
        except TimeoutError:
            ok = False
        last = ctx.ledger("request", "session/prompt")[-1]
        ctx.check("restore.prompt_reaches_restored_session", ok and last["sessionId"] == "saved-orchard", last.get("sessionId"))
        ctx.check("restore.exactly_one_load", len(loads) == 1, len(loads))
    t.capture("final")
    pids = {r["pid"] for r in ctx.ledger("spawn")}
    quit_fg(t)
    time.sleep(1)
    ctx.check("restore.all_servers_stopped_on_quit", not [p for p in pids if running(p)], sorted(pids))


def s32_dismiss_reaps_prepared(ctx, cmd):
    """Esc on the restore picker discards the prepared server; the current session keeps working."""
    t = start_fg(ctx)
    submit(t, "hello lantern")
    t.wait_for("heard: hello lantern", 20)
    first_pid = ctx.ledger("spawn")[0]["pid"]
    open_resume_rows(t, cmd)
    t.capture("rows")
    prepared = [r["pid"] for r in ctx.ledger("spawn") if r["pid"] != first_pid]
    t.keys("Escape")
    time.sleep(2.5)
    t.capture("dismissed")
    ctx.check("dismiss.prepared_server_started", bool(prepared), prepared)
    ctx.check("dismiss.prepared_server_reaped", prepared and not [p for p in prepared if running(p)],
              [(p, running(p)) for p in prepared])
    submit(t, "still lit")
    try:
        t.wait_for("heard: still lit", 15)
        ok = True
    except TimeoutError:
        ok = False
    ctx.check("dismiss.current_session_keeps_working", ok and ctx.ledger("request", "session/prompt")[-1]["pid"] == first_pid,
              last_nonblank(t.screen()))
    quit_fg(t)


def s33_new_through_host(ctx, refuse=False):
    """/new starts the next session on a fresh server in the foreground session's folder; a refusal keeps the current one."""
    t = start_fg(ctx)
    submit(t, "hello lantern")
    t.wait_for("heard: hello lantern", 20)
    beta = str(ctx.fx["roots"]["beta"])
    slash(t, f"/new {beta}", settle=4)
    submit(t, "orchard word")
    t.wait_for("heard: orchard word", 20)
    fg_new = ctx.ledger("request", "session/new")[-1]
    ctx.check("new.dir_argument_sets_session_folder", fg_new.get("cwd") == beta, fg_new.get("cwd"))
    time.sleep(1.0)
    if refuse:
        (ctx.fx["control"] / "new-refuse").write_text("1")
    mark = len(ctx.ledger())
    slash(t, "/new", settle=4)
    t.capture("after-new")
    later = ctx.ledger()[mark:]
    news = [r for r in later if r.get("method") == "session/new"]
    restored_pid = fg_new["pid"]
    if fg_new.get("cwd") == beta:  # otherwise the launch folder and the session folder are the same and the check is vacuous
        ctx.check("new.uses_foreground_session_folder", news and news[-1].get("cwd") == beta, [r.get("cwd") for r in news])
    ctx.check("new.on_a_fresh_server_process", news and news[-1]["pid"] != restored_pid, [(r["pid"], restored_pid) for r in news])
    if refuse:
        ctx.check("new.refusal_message_shown", "no fresh lantern is available" in " ".join(t.screen().split()),
                  [l.strip() for l in t.screen().splitlines() if "lantern" in l][-3:])
        submit(t, "still the orchard")
        try:
            t.wait_for("heard: still the orchard", 15)
            ok = True
        except TimeoutError:
            ok = False
        last = ctx.ledger("request", "session/prompt")[-1]
        ctx.check("new.refusal_keeps_current_session", ok and last["pid"] == restored_pid, (last["pid"], restored_pid))
    else:
        submit(t, "first fresh word")
        try:
            t.wait_for("heard: first fresh word", 15)
            ok = True
        except TimeoutError:
            ok = False
        last = ctx.ledger("request", "session/prompt")[-1]
        ctx.check("new.prompt_reaches_new_session", ok and news and last["pid"] == news[-1]["pid"], last.get("sessionId"))
        ctx.check("new.previous_server_still_running", running(restored_pid), restored_pid)
    t.capture("final")
    pids = {r["pid"] for r in ctx.ledger("spawn")}
    quit_fg(t)
    time.sleep(1)
    ctx.check("new.all_servers_stopped_on_quit", not [p for p in pids if running(p)], sorted(pids))


EARLY = "early lantern word"
STAGE_DELAY = 6.0


def s33_early_input(ctx, refuse=False, stage="init"):
    """Input typed while the /new server is still in `stage` (initialize or session/new) goes only to the new session; on refusal it stays unsent."""
    t = start_fg(ctx)
    submit(t, "hello lantern")
    t.wait_for("heard: hello lantern", 20)
    first_pid = ctx.ledger("spawn")[0]["pid"]
    (ctx.fx["control"] / f"{stage}-delay").write_text(str(STAGE_DELAY))
    if refuse:
        (ctx.fx["control"] / "new-refuse").write_text("1")
    t.type("/new")
    time.sleep(1.0)
    t.keys("Enter")
    time.sleep(0.15)
    if re.search(r"^\s*›?\s*/new\s*$", t.screen(), re.M):
        t.keys("Enter")
    wait_ledger(ctx, lambda r: r.get("kind") == f"{stage}_delay_start", timeout=10)
    t.type(EARLY)
    time.sleep(1.0)
    t.keys("Enter")
    typed_at = time.time()
    t.capture("typed-during-init")
    starts = ctx.ledger(f"{stage}_delay_start")
    delayed_pid = starts[0]["pid"] if starts else None
    wait_ledger(ctx, lambda r: r.get("kind") == f"{stage}_delay_end", timeout=STAGE_DELAY + 5)
    ends = ctx.ledger(f"{stage}_delay_end")
    ctx.check(f"early.typed_during_{stage}",
              bool(starts and ends and starts[0]["t"] < typed_at < ends[0]["t"] and delayed_pid != first_pid),
              {"start": starts and starts[0]["t"], "typed": typed_at, "end": ends and ends[0]["t"], "pid": delayed_pid})
    early = lambda: [r for r in ctx.ledger("request", "session/prompt") if r.get("text", "").endswith(EARLY)]
    if refuse:
        wait_ledger(ctx, lambda r: r.get("kind") == "new_refused", timeout=10)
        time.sleep(4)
        t.capture("after-refusal")
        screen = t.screen()
        ctx.check("early.refusal_recorded_by_adapter", bool(ctx.ledger("new_refused")), len(ctx.ledger("new_refused")))
        ctx.check("early.refused_input_stays_unsent", not early(), [(r["pid"], r["t"]) for r in early()])
        ctx.check("early.refusal_message_shown", "no fresh lantern is available" in " ".join(screen.split()),
                  [l.strip() for l in screen.splitlines() if "lantern" in l][-3:])
        composer = [l.strip() for l in screen.splitlines() if l.strip().startswith("›")][-1:]
        ctx.check("early.refused_input_back_in_composer", composer == [f"› {EARLY}"], composer)
        time.sleep(1.0)
        t.keys("Enter")
        try:
            t.wait_for(f"heard: {EARLY}", 15)
        except TimeoutError:
            pass
        rows = early()
        ctx.check("early.enter_sends_held_input_once_to_previous_session",
                  len(rows) == 1 and rows[0]["pid"] == first_pid, [(r["pid"], first_pid) for r in rows])
    else:
        wait_ledger(ctx, lambda r: r.get("method") == "session/prompt" and r.get("text", "").endswith(EARLY), timeout=20)
        time.sleep(3)
        rows = early()
        news = [r for r in ctx.ledger("request", "session/new") if r["pid"] == delayed_pid]
        ctx.check("early.delivered_once", len(rows) == 1, [(r["pid"], r["t"]) for r in rows])
        ctx.check("early.reaches_the_new_session", bool(rows) and rows[0]["pid"] == delayed_pid,
                  [(r["pid"], delayed_pid, first_pid) for r in rows])
        ctx.check("early.sent_after_the_new_session_exists", bool(rows and news) and rows[0]["t"] > news[0]["t"],
                  {"prompt": rows and rows[0]["t"], "session_new": news and news[0]["t"]})
        ctx.check("early.previous_session_gets_only_its_own_prompt",
                  len([r for r in ctx.ledger("request", "session/prompt") if r["pid"] == first_pid]) == 1, first_pid)
        ctx.check("early.answered_on_screen", f"heard: {EARLY}" in t.screen(), last_nonblank(t.screen()))
    t.capture("final")
    quit_fg(t)


SCENARIOS = {
    "S01": ("foreground launch and exit", s01_foreground_launch_exit),
    "S02": ("foreground quit stops a running turn", s02_foreground_quit_stops_turn),
    "S03a": ("attach detach by Ctrl+D keeps the turn", lambda c: s03_attach_detach_keeps_turn(c, "ctrl-d")),
    "S03b": ("attach detach by /quit keeps the turn", lambda c: s03_attach_detach_keeps_turn(c, "quit")),
    "S03c": ("attach PTY hangup keeps the turn", lambda c: s03_attach_detach_keeps_turn(c, "hangup")),
    "S04": ("/close ends the session", s04_close_ends_session),
    "S05": ("slash discovery and greyed commands", s05_slash_discovery),
    "S06": ("folder inheritance, override and completion", s06_cwd_inheritance),
    "S07": ("remote @ completion per session", s07_remote_at_completion),
    "S08": ("roles, watcher Enter, transfer", s08_roles_and_drafts),
    "S09": ("reconnect keeps drafts", s09_reconnect_keeps_drafts),
    "S10": ("approval modes", s10_approvals),
    "S11a": ("Agent Home in plain nori", lambda c: s11_agent_home_regressions(c, "fg")),
    "S11b": ("Agent Home in attach", lambda c: s11_agent_home_regressions(c, "attach")),
    "S12": ("new sessions take the host default mode", s12_new_takes_host_default),
    "S13": ("approvals per foreground session and role", s13_approvals_per_session),
    "S14": ("host-scoped picker and agent-server resume", s14_picker_scope_and_resume),
    "S15a": ("legacy Full Access migrates", lambda c: s15_legacy_approval_migration(c, "full-access")),
    "S15b": ("legacy Read Only migrates to Ask", lambda c: s15_legacy_approval_migration(c, "read-only")),
    "S15c": ("legacy Read Only without Ask migrates to Agent",
             lambda c: s15_legacy_approval_migration(c, "read-only-no-ask")),
    "S16": ("watcher reconnect keeps an unsent draft", s16_watcher_reconnect_keeps_draft),
    "S17": ("silent connection loss", s17_silent_loss),
    "S18a": ("thought text in the working indicator (plain)", lambda c: s18_thought_indicator(c, "fg")),
    "S18b": ("thought text in the working indicator (attach)", lambda c: s18_thought_indicator(c, "attach")),
    "S19a": ("tool-output cap (plain)", lambda c: s19_tool_output_cap(c, "fg", True)),
    "S19b": ("tool-output cap (attach)", lambda c: s19_tool_output_cap(c, "attach", True)),
    "S19c": ("tool output full by default (plain)", lambda c: s19_tool_output_cap(c, "fg", False)),
    "S19d": ("tool output full by default (attach)", lambda c: s19_tool_output_cap(c, "attach", False)),
    "S20a": ("cancel an agent-started turn (plain, Esc)", lambda c: s20_agent_started_cancel(c, "fg")),
    "S20b": ("cancel an agent-started turn (attach, Ctrl+C)", lambda c: s20_agent_started_cancel(c, "attach")),
    "S21a": ("plain nori owns a private host; hangup ends every session", lambda c: s21_foreground_owns_host(c, "hangup")),
    "S21b": ("plain nori: SIGTERM ends every session and restores the terminal", lambda c: s21_foreground_owns_host(c, "sigterm")),
    "S21c": ("plain nori beside a background host", s21c_private_beside_background_host),
    "S22": ("switch agent servers in plain nori", s22_switch_agents),
    "S23": ("client-local commands in plain nori", s23_local_commands),
    "S24": ("picker-first `nori resume` on the owned host", s24_picker_first_resume),
    "S25a": ("permission answered once (plain)", lambda c: s25_permission_attribution(c, "fg")),
    "S25b": ("permission: controller answers, watcher cannot (attach)", lambda c: s25_permission_attribution(c, "attach")),
    "S26a": ("queued follow-up delivered once (plain)", lambda c: s26_queued_followup(c, "deliver")),
    "S26b": ("queued follow-up reclaimed, never sent (plain)", lambda c: s26_queued_followup(c, "reclaim")),
    "S26c": ("paused queue resumed, delivered once (plain)", lambda c: s26_queued_followup(c, "resume")),
    "S27": ("standalone nori-tui binary", s27_standalone_nori_tui),
    "S28a": ("Escape cancels once (plain)", lambda c: s28_escape_cancel(c, "fg")),
    "S28b": ("Escape cancels once (attach)", lambda c: s28_escape_cancel(c, "attach")),
    "S29a": ("refused /close (plain)", lambda c: s29_close_refusal(c, "fg")),
    "S29b": ("refused /close (attach)", lambda c: s29_close_refusal(c, "attach")),
    "S29c": ("refused /close with two attached terminals", s29c_close_refusal_two_clients),
    "S29d": ("second /close while a refused first is in flight", s29d_second_close_while_refusal_pending),
    "S30": ("prepared agent connection over the wire", s30_prepared_agent),
    "S31a": ("/resume restores an agent server's saved row via the host", lambda c: s31_restore_through_host(c, "/resume")),
    "S31b": ("/agent restores another server's saved row via the host", lambda c: s31_restore_through_host(c, "/agent")),
    "S31c": ("refused restore keeps the current session", lambda c: s31_restore_through_host(c, "/resume", refuse=True)),
    "S31d": ("refused /agent restore keeps the current session", lambda c: s31_restore_through_host(c, "/agent", refuse=True)),
    "S33a": ("/new through the owned host in the session folder", s33_new_through_host),
    "S33b": ("refused /new keeps the current session", lambda c: s33_new_through_host(c, refuse=True)),
    "S33c": ("input typed while the /new server initializes reaches only the new session", s33_early_input),
    "S33d": ("input typed while a refused /new initializes stays unsent", lambda c: s33_early_input(c, refuse=True)),
    "S33e": ("input typed while session/new is pending reaches only the new session",
             lambda c: s33_early_input(c, stage="new")),
    "S33f": ("input typed while a refused session/new is pending stays unsent",
             lambda c: s33_early_input(c, refuse=True, stage="new")),
    "S32a": ("Esc on /resume rows reaps the prepared server", lambda c: s32_dismiss_reaps_prepared(c, "/resume")),
    "S32b": ("Esc on /agent rows reaps the prepared server", lambda c: s32_dismiss_reaps_prepared(c, "/agent")),
}


CAP = "[tui]\ntool_output_max_lines = 10"
FIXTURE_ARGS = {"S12": {"host_extra": 'default_mode = "ask"'}, "S19a": {"top_extra": CAP}, "S19b": {"top_extra": CAP}}


def main():
    only = [s for s in ARGS.only.split(",") if s]
    for sid, (title, fn) in SCENARIOS.items():
        if only and sid not in only:
            continue
        print(f"== {sid} {title}", flush=True)
        ctx = Ctx(sid, **FIXTURE_ARGS.get(sid, {}))
        try:
            fn(ctx)
        except Exception as error:  # a scenario that cannot reach its state is a failed check
            ctx.check("scenario.completed", False, f"{type(error).__name__}: {error}")
            (ctx.dir / "error.txt").write_text(traceback.format_exc())
            for t in ctx.terms:
                try:
                    t.capture("at-error")
                except Exception:
                    pass
        finally:
            ctx.finish()
    term.kill_server()
    passed = sum(r["passed"] for r in RESULTS)
    summary = {"nori": str(Path(ARGS.nori).resolve().name), "passed": passed, "failed": len(RESULTS) - passed, "checks": len(RESULTS)}
    (OUT / "summary.json").write_text(json.dumps(summary, indent=2))
    print(json.dumps(summary))


if __name__ == "__main__":
    main()
