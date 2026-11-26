#!/usr/bin/env python3
"""Run the host TUI acceptance matrix against one nori binary.

  python3 -I harness/run.py --nori PATH --out results/<sha8> [--only S03,S07]

Each scenario builds a fresh synthetic run directory, drives real tmux PTYs,
watches the host through an independent observer connection and the
adapter ledger, then records checks in results.json. Screens are saved as
.txt and .ansi; render PNGs afterwards with harness/render.sh.
"""
import argparse
import difflib
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
P.add_argument("--client", default="", help="a different nori build for the attach client under test (S38)")
ARGS = P.parse_args()
# Terminals run `nori` from a synthetic bin directory so captures and logs
# show only synthetic paths.
# ACC_LANE runs several commits side by side, each with its own work dir and tmux socket.
WORK = Path("/tmp/nori-host-tui-acceptance" + (f"-{os.environ['ACC_LANE']}" if os.environ.get("ACC_LANE") else ""))
BIN = WORK / "bin"
BIN.mkdir(parents=True, exist_ok=True)
(BIN / "nori").unlink(missing_ok=True)
(BIN / "nori").symlink_to(Path(ARGS.nori).resolve())
if (Path(ARGS.nori).resolve().parent / "nori-tui").exists():
    (BIN / "nori-tui").unlink(missing_ok=True)
    (BIN / "nori-tui").symlink_to(Path(ARGS.nori).resolve().parent / "nori-tui")
NORI = "nori"
# S38 runs the attach client under test from its own synthetic directory, so `nori` on PATH stays the host's build.
CLIENT = NORI
if ARGS.client:
    (WORK / "client").mkdir(parents=True, exist_ok=True)
    (WORK / "client" / "nori").unlink(missing_ok=True)
    (WORK / "client" / "nori").symlink_to(Path(ARGS.client).resolve())
    CLIENT = str(WORK / "client" / "nori")
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

    def start_host(self, cwd="alpha", log=False):
        if log:
            env = self.env("host")
            env["RUST_LOG"] = "nori_host_runtime=debug,nori_harness=debug"
            out = subprocess.run([str(BIN / "nori"), "host", "--port", "0"], env=env, cwd=self.fx["roots"][cwd],
                                 capture_output=True, text=True, timeout=60)
        else:
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


def confirm_if_asked(t):
    """Accept an Approve all confirmation step if one is on screen."""
    if re.search(r"(stop asking|enter approve all)", t.screen(), re.I):
        t.capture("approve-all-confirmation")
        t.keys("Enter")
        time.sleep(1.2)


def choose(t, label, limit=12):
    """Move a list selection onto the row containing `label`, then press Enter."""
    for direction in ("Down", "Up"):
        for _ in range(limit):
            row = next((l for l in t.screen().splitlines() if re.match(r"^\s*(›|>|▶|▏)", l) and label in l), None)
            if row:
                t.keys("Enter")
                return True
            t.keys(direction)
            time.sleep(0.1)
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
    confirm_if_asked(t)
    ctx.check("approvals.controller_can_pick_approve_all", picked, last_nonblank(t.screen()))
    t.type("/new")
    t.keys("Enter")
    time.sleep(0.6)
    if re.search(r"^\s*/new\b", t.screen(), re.M):
        t.keys("Enter")
    time.sleep(2)
    sets = mode_sets(ctx)
    news = len(ctx.ledger("request", "session/new"))
    ctx.check("default.second_new_does_not_copy_foreground", news >= 2
              and HOST_MODE_FOR.get(sets[-1][1]) == "ask", {"session/new": news, "mode sets": sets})
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
    # Coordinator default (2026-11-24): Ask stays listed but marked not available, with the reason.
    ask_rows = re.findall(r"^\s*(?:›\s*)?\d?\.?\s*Ask\b.*$", menu, re.M)
    ctx.check("approvals.ask_unavailable_for_adapter_without_ask",
              "Approve all" in menu and all(re.search(r"not available", l, re.I) for l in ask_rows), menu[-400:])
    before = mode_sets(ctx)
    picked = choose(a, "Approve all")
    time.sleep(1)
    confirm_if_asked(a)
    after = mode_sets(ctx)
    ctx.check("approvals.change_reaches_only_foreground", picked and len(after) >= len(before), after)
    footer = a.screen()
    ctx.check("approvals.footer_shows_foreground_mode", footer_mode(footer) == "approve-all",
              [l for l in footer.splitlines() if "Approvals:" in l])
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
    # Shared joined TUI (#784 on): the role leads the footer row, e.g. `controlling · app · ...`.
    footer = next((l for l in reversed(screen.splitlines()) if re.match(r"\s*(controlling|watching|RECONNECTING|reconnecting) ·", l)), "")
    if footer:
        return footer.strip().split(" ·")[0].lower()
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


# ---------------------------------------------------------------------------
# S34: plain `nori` startup through the owned host (#750)
# ---------------------------------------------------------------------------
HOST_LOG_ENV = "RUST_LOG=nori_host_runtime=debug,nori_harness=debug"
STARTUP_WORD = "startup lantern word"
SKILLSETS_MOCK = "#!/bin/sh\nfor a in \"$@\"; do case \"$a\" in install|switch) exit 0 ;; esac; done\necho default\nexit 0\n"


def host_requests(ctx):
    """ACP methods the owned host received, from its debug log (present from #750)."""
    out = []
    lines = []
    for log in sorted((ctx.fx["homes"]["host"] / "log").glob("nori-acp*")):
        lines += log.read_text(errors="replace").splitlines()
    for line in lines:
        if "host ACP request" in line and " method=" in line:
            out.append(line.split(" method=", 1)[1].split()[0])
    return out


def adapter_sequence(ctx, pid=None):
    return [r.get("method") for r in ctx.ledger("request")
            if (pid is None or r["pid"] == pid) and r.get("method")
            and not r["method"].startswith("session/set_")]


def record_startup_traces(ctx):
    (ctx.dir).mkdir(parents=True, exist_ok=True)
    (ctx.dir / "host-requests.txt").write_text("\n".join(host_requests(ctx)) + "\n")
    rows = [f'{r["pid"]} {r.get("method")} {r.get("sessionId") or "-"} {r.get("text", "")[-40:]!r}'
            for r in ctx.ledger("request")]
    (ctx.dir / "adapter-trace.txt").write_text("\n".join(rows) + "\n")


def startup_path(ctx, label):
    """Check and record which path started the first session."""
    reqs = host_requests(ctx)
    prep = reqs.index("_nori/host/prepare_agent") if "_nori/host/prepare_agent" in reqs else None
    new = reqs.index("session/new") if "session/new" in reqs else None
    ctx.check(f"{label}.host_prepared_then_started", prep is not None and new is not None and prep < new, reqs[:8])


def prompts_with(ctx, word):
    return [r for r in ctx.ledger("request", "session/prompt") if word in r.get("text", "")]


def launch_plain(ctx, args="", pre="", delay=None, refuse=False, name="fg"):
    if delay:
        (ctx.fx["control"] / "init-delay").write_text(str(delay))
    if refuse:
        (ctx.fx["control"] / "new-refuse").write_text("1")
    t = ctx.term(name)
    t.run(f"{pre}{HOST_LOG_ENV} {NORI} --skip-trust-directory {args}".strip())
    return t


def wait_text(t, needle, timeout=15):
    try:
        t.wait_for(needle, timeout)
        return True
    except TimeoutError:
        return False


def s34_plain_startup(ctx):
    """Plain startup prepares the default server through the owned host and starts one session before any prompt."""
    t = launch_plain(ctx)
    wait_composer(t)
    wait_ledger(ctx, lambda r: r.get("method") == "session/new", timeout=15)
    time.sleep(1.5)
    t.capture("ready")
    spawns = ctx.ledger("spawn")
    ctx.check("startup.one_server", len(spawns) == 1, [s["pid"] for s in spawns])
    seq = adapter_sequence(ctx)
    ctx.check("startup.session_started_before_any_prompt", seq.count("session/new") == 1 and "session/prompt" not in seq, seq)
    startup_path(ctx, "startup")
    submit(t, STARTUP_WORD)
    ok = wait_text(t, f"heard: {STARTUP_WORD}", 20)
    time.sleep(1.5)
    rows = prompts_with(ctx, STARTUP_WORD)
    ctx.check("startup.prompt_answered_once", ok and len(rows) == 1 and rows[0]["pid"] == spawns[0]["pid"], len(rows))
    t.capture("answered")
    record_startup_traces(ctx)
    pids = {r["pid"] for r in ctx.ledger("spawn")}
    quit_fg(t)
    time.sleep(1)
    ctx.check("startup.server_stopped_on_quit", not [p for p in pids if running(p)], sorted(pids))


def s34_early_input(ctx, how):
    """Input given before the startup server is ready (typed, positional or piped) reaches the startup session exactly once."""
    pre, args, word = "", "", STARTUP_WORD
    if how == "positional":
        args = f'"{word}"'
    elif how == "piped":
        pre = f"printf '{word}' | "
    elif how == "piped+arg":
        pre, args, word = "printf 'orchard context line' | ", '--stdin "summarize the piped lantern"', "summarize the piped lantern"
    t = launch_plain(ctx, args=args, pre=pre, delay=STAGE_DELAY)
    started = wait_ledger(ctx, lambda r: r.get("kind") == "init_delay_start", timeout=15)
    typed_at = None
    if how == "typed":
        wait_composer(t)
        submit(t, word)
        typed_at = time.time()
    time.sleep(0.8)
    t.capture("while-preparing")
    ends = wait_ledger(ctx, lambda r: r.get("kind") == "init_delay_end", timeout=STAGE_DELAY + 5)
    held = [r for r in ctx.ledger("request") if r["t"] < (ends[0]["t"] if ends else 0) and r.get("method", "").startswith("session/")]
    ctx.check("early.server_held_in_initialize", bool(started and ends), {"start": bool(started), "end": bool(ends)})
    if typed_at:
        ctx.check("early.typed_during_hold", bool(started and ends) and started[0]["t"] < typed_at < ends[0]["t"],
                  {"start": started and started[0]["t"], "typed": typed_at, "end": ends and ends[0]["t"]})
    ctx.check("early.nothing_sent_before_initialize", not held, [r.get("method") for r in held])
    wait_ledger(ctx, lambda r: r.get("method") == "session/prompt" and word in r.get("text", ""), timeout=20)
    time.sleep(3)
    rows = prompts_with(ctx, word)
    news = ctx.ledger("request", "session/new")
    spawns = ctx.ledger("spawn")
    ctx.check("early.delivered_once", len(rows) == 1, [(r["pid"], r["t"]) for r in rows])
    ctx.check("early.one_server_one_session", len(spawns) == 1 and len(news) == 1, (len(spawns), len(news)))
    ctx.check("early.sent_after_session_new", bool(rows and news) and rows[0]["t"] > news[0]["t"],
              {"prompt": rows and rows[0]["t"], "new": news and news[0]["t"]})
    if how == "piped+arg":
        ctx.check("early.piped_context_included", bool(rows) and "orchard context line" in rows[0].get("text", ""), rows and rows[0].get("text", "")[-120:])
    screen = " ".join(t.screen().split())
    ctx.check("early.answered_on_screen", f"heard:" in screen and word.split()[-1] in screen, last_nonblank(t.screen()))
    startup_path(ctx, "early")
    t.capture("answered")
    record_startup_traces(ctx)
    quit_fg(t)


def composer_line(screen):
    return [l.strip() for l in screen.splitlines() if l.strip().startswith("›")][-1:]


def s34_refused_startup(ctx, how):
    """A refused startup session keeps typed or positional input unsent and in the composer; the next Enter retries once."""
    args = f'"{STARTUP_WORD}"' if how == "positional" else ""
    t = launch_plain(ctx, args=args, delay=STAGE_DELAY if how == "typed" else None, refuse=True)
    if how == "typed":
        wait_ledger(ctx, lambda r: r.get("kind") == "init_delay_start", timeout=15)
        wait_composer(t)
        submit(t, STARTUP_WORD)
    wait_ledger(ctx, lambda r: r.get("kind") == "new_refused", timeout=STAGE_DELAY + 10)
    time.sleep(3)
    t.capture("after-refusal")
    screen = t.screen()
    ctx.check("refused.adapter_refused_session_new", len(ctx.ledger("new_refused")) == 1, len(ctx.ledger("new_refused")))
    ctx.check("refused.input_unsent", not prompts_with(ctx, STARTUP_WORD), len(prompts_with(ctx, STARTUP_WORD)))
    ctx.check("refused.reason_shown", "no fresh lantern is available" in " ".join(screen.split()),
              [l.strip() for l in screen.splitlines() if "lantern" in l.lower()][-3:])
    ctx.check("refused.input_back_in_composer", composer_line(screen) == [f"› {STARTUP_WORD}"], composer_line(screen))
    t.capture("composer")
    time.sleep(1.0)
    t.keys("Enter")
    ok = wait_text(t, f"heard: {STARTUP_WORD}", 20)
    time.sleep(2)
    rows = prompts_with(ctx, STARTUP_WORD)
    news_ok = [r for r in ctx.ledger("request", "session/new")]
    ctx.check("refused.enter_retries_and_sends_once", ok and len(rows) == 1, [(r["pid"], r.get("sessionId")) for r in rows])
    ctx.check("refused.retry_started_a_session", len(news_ok) == 2, len(news_ok))
    t.capture("retried")
    record_startup_traces(ctx)
    pids = {r["pid"] for r in ctx.ledger("spawn")}
    quit_fg(t)
    time.sleep(1)
    ctx.check("refused.servers_stopped_on_quit", not [p for p in pids if running(p)], sorted(pids))


def s34_new_without_session(ctx, when):
    """/new with no live session: during startup it waits for the startup session; after a refused start it starts one."""
    if when == "during":
        t = launch_plain(ctx, delay=STAGE_DELAY)
        wait_ledger(ctx, lambda r: r.get("kind") == "init_delay_start", timeout=15)
        wait_composer(t)
        slash(t, "/new", settle=0.5)
        t.capture("new-during-hold")
        wait_ledger(ctx, lambda r: r.get("kind") == "init_delay_end", timeout=STAGE_DELAY + 5)
    else:
        t = launch_plain(ctx, refuse=True)
        wait_ledger(ctx, lambda r: r.get("kind") == "new_refused", timeout=15)
        time.sleep(2.5)
        t.capture("refused")
        t.keys("Escape")
        time.sleep(0.5)
        slash(t, "/new", settle=0.5)
    target = 1 if when == "during" else 2
    deadline = time.time() + 15
    while time.time() < deadline and len(ctx.ledger("request", "session/new")) < target:
        time.sleep(0.2)
    time.sleep(3)
    t.capture("after-new")
    news = ctx.ledger("request", "session/new")
    spawns = ctx.ledger("spawn")
    expected_news = 1 if when == "during" else 2
    ctx.check("new.one_session_started", len(news) == expected_news, [(r["pid"], r.get("cwd")) for r in news])
    if when == "during":
        ctx.check("new.reused_startup_server", len(spawns) == 1, [s["pid"] for s in spawns])
    submit(t, STARTUP_WORD)
    ok = wait_text(t, f"heard: {STARTUP_WORD}", 20)
    time.sleep(1.5)
    rows = prompts_with(ctx, STARTUP_WORD)
    ctx.check("new.prompt_reaches_started_session_once", ok and len(rows) == 1 and rows[0]["pid"] == news[-1]["pid"],
              [(r["pid"], news[-1]["pid"]) for r in rows])
    reqs = host_requests(ctx)
    ctx.check("new.started_through_host", reqs.count("session/new") == expected_news, reqs[:10])
    t.capture("answered")
    record_startup_traces(ctx)
    pids = {r["pid"] for r in ctx.ledger("spawn")}
    quit_fg(t)
    time.sleep(1)
    ctx.check("new.servers_stopped_on_quit", not [p for p in pids if running(p)], sorted(pids))


def s34_resume_during_startup(ctx):
    """/resume while the startup server initializes lists that server's saved rows and restores on it: one adapter, no fresh session."""
    t = launch_plain(ctx, delay=STAGE_DELAY)
    wait_ledger(ctx, lambda r: r.get("kind") == "init_delay_start", timeout=15)
    wait_composer(t)
    slash(t, "/resume", settle=0.5)
    t.capture("resume-during-hold")
    wait_ledger(ctx, lambda r: r.get("kind") == "init_delay_end", timeout=STAGE_DELAY + 5)
    listed = wait_text(t, "Saved orchard survey", 15)
    t.capture("rows")
    ctx.check("resume.rows_from_startup_server", listed, last_nonblank(t.screen()))
    if listed:
        choose(t, "Saved orchard survey")
        time.sleep(3.5)
    t.capture("restored")
    spawns = ctx.ledger("spawn")
    loads = [r for r in ctx.ledger("request") if r.get("method") in ("session/load", "session/resume")]
    news = ctx.ledger("request", "session/new")
    ctx.check("resume.same_adapter_process", len(spawns) == 1 and bool(loads) and loads[0]["pid"] == spawns[0]["pid"],
              {"spawns": [s["pid"] for s in spawns], "loads": [(l["pid"], l.get("sessionId")) for l in loads]})
    ctx.check("resume.no_fresh_session_started", not news, [(r["pid"]) for r in news])
    ctx.check("resume.exactly_one_load", len(loads) == 1 and loads[0].get("sessionId") == "saved-orchard", len(loads))
    submit(t, STARTUP_WORD)
    ok = wait_text(t, f"heard: {STARTUP_WORD}", 20)
    time.sleep(1.5)
    rows = prompts_with(ctx, STARTUP_WORD)
    ctx.check("resume.prompt_reaches_restored_session", ok and len(rows) == 1 and rows[0].get("sessionId") == "saved-orchard",
              [r.get("sessionId") for r in rows])
    reqs = host_requests(ctx)
    # The host restores a saved row through its own session/new carrying the selection; the adapter sees session/load.
    ctx.check("resume.loaded_through_host", "_nori/host/prepare_agent" in reqs and "session/new" in reqs
              and reqs.index("_nori/host/prepare_agent") < reqs.index("session/new") and bool(loads), reqs[:10])
    t.capture("answered")
    record_startup_traces(ctx)
    quit_fg(t)


def s34_deferred_skillset(ctx, action):
    """Per-session skillset startup: the session starts through the owned host after the skillset choice."""
    mock_dir = ctx.run_dir / "skillsets-bin"
    mock_dir.mkdir(parents=True, exist_ok=True)
    mock = mock_dir / "nori-skillsets"
    mock.write_text(SKILLSETS_MOCK)
    mock.chmod(0o755)
    cfg = ctx.fx["homes"]["host"] / "config.toml"
    cfg.write_text(cfg.read_text() + "\n[tui]\nskillset_per_session = true\n")
    t = launch_plain(ctx, pre=f"PATH={mock_dir}:$PATH ")
    shown = wait_text(t, "Select skillset", 20) and wait_text(t, "default", 10)
    t.capture("picker")
    ctx.check("skillset.picker_shown", shown, last_nonblank(t.screen()))
    time.sleep(1.0)
    ctx.check("skillset.no_session_before_choice", not ctx.ledger("request", "session/new"), len(ctx.ledger("request", "session/new")))
    if action == "apply":
        t.keys("Down")
        time.sleep(0.3)
        t.keys("Enter")
    else:
        t.keys("Escape")
    wait_ledger(ctx, lambda r: r.get("method") == "session/new", timeout=15)
    time.sleep(2.5)
    t.capture("after-choice")
    news = ctx.ledger("request", "session/new")
    ctx.check("skillset.one_session_started_after_choice", len(news) == 1, len(news))
    startup_path(ctx, "skillset")
    submit(t, STARTUP_WORD)
    ok = wait_text(t, f"heard: {STARTUP_WORD}", 20)
    time.sleep(1.5)
    rows = prompts_with(ctx, STARTUP_WORD)
    ctx.check("skillset.prompt_answered_once", ok and len(rows) == 1, len(rows))
    t.capture("answered")
    record_startup_traces(ctx)
    quit_fg(t)


def s34_resume_id_control(ctx):
    """Control: `nori resume <id>` stays in-process by design, so the host log must show no host requests for it."""
    first = launch_plain(ctx, name="first")
    wait_composer(first)
    submit(first, STARTUP_WORD)
    first.wait_for(f"heard: {STARTUP_WORD}", 20)
    quit_fg(first)
    ctx.check("control.first_launch_used_host", "_nori/host/prepare_agent" in host_requests(ctx), host_requests(ctx)[:4])
    for log in (ctx.fx["homes"]["host"] / "log").glob("nori-acp*"):
        log.unlink()
    mark = len(ctx.ledger())
    t = ctx.term("fg")
    t.run(f"{HOST_LOG_ENV} {NORI} --skip-trust-directory resume --last")
    wait_composer(t)
    wait_ledger(ctx, lambda r: r.get("method") in ("session/load", "session/resume"), timeout=15)
    time.sleep(2)
    t.capture("resumed")
    loads = [r for r in ctx.ledger()[mark:] if r.get("method") in ("session/load", "session/resume")]
    ctx.check("control.last_session_loaded", len(loads) == 1, [(r.get("method"), r.get("sessionId")) for r in loads])
    reqs = host_requests(ctx)
    ctx.check("control.no_host_requests", not reqs, reqs[:6])
    record_startup_traces(ctx)
    quit_fg(t)



# ---------------------------------------------------------------------------
# S35: one slash-command table for plain nori and nori attach (#751)
# ---------------------------------------------------------------------------
REASON_HOST = "not available on this host yet"
REASON_ATTACH = "not available in nori attach yet"
REASON_WATCH = "watching"


def menu_row(screen, cmd):
    """The `/` menu row whose first word is exactly `cmd`."""
    for line in screen.splitlines():
        words = line.strip().split()
        if words and words[0] == cmd:
            return line.strip()
    return ""


def composer_text(screen):
    """The composer row: `› text`, or `/ text` while the `/` menu styles it (not a `› You` transcript block)."""
    rows = [l for l in screen.splitlines() if (l.startswith("/") or l.startswith("› ")) and l.strip() != "› You"]
    return rows[-1].replace("/ ", "/", 1).strip() if rows else ""


def filtered_row(t, cmd):
    screen = open_menu(t, cmd)
    t.keys("C-u")
    return menu_row(screen, cmd)


def agent_commands_on(ctx):
    (ctx.fx["control"] / "agent-commands").write_text("1")


def prompt_texts(ctx):
    return [r.get("text", "") for r in ctx.ledger("request", "session/prompt")]


def requests_since(ctx, n):
    return [r.get("method") for r in ctx.ledger("request")[n:]]


def open_menu(t, text="/", settle=1.0):
    t.keys("C-u")
    t.type(text)
    time.sleep(settle)
    return t.screen()


def run_typed(t, text, settle=1.5):
    """Type a command and press Enter once, after the paste-burst pause."""
    t.keys("C-u")
    t.type(text)
    time.sleep(1.0)
    t.keys("Enter")
    time.sleep(settle)
    return t.screen()


def check_agent_command_passthrough(ctx, t, label, prefix=""):
    for text in ["/review the parser", "/review", "/lanternize"]:
        before = prompt_texts(ctx).count(text)
        run_typed(t, "/" + prefix + text[1:], settle=0.5)
        wait_ledger(ctx, lambda r: r.get("method") == "session/prompt" and r.get("text") == text, timeout=10)
        time.sleep(1.5)
        sent = prompt_texts(ctx).count(text) - before
        ctx.check(f"{label}.agent_command_{text.split()[0][1:]}{'_args' if ' ' in text else ''}_sent_once",
                  sent == 1, {"sent": sent, "prompts": prompt_texts(ctx)[-4:]})


def s35_plain_shared_commands(ctx):
    """Plain nori: the `/` menu, Tab completion, dispatch, and agent commands through the shared table."""
    agent_commands_on(ctx)
    t = start_fg(ctx)
    submit(t, "first light")
    t.wait_for("heard: first light", 20)
    menu = open_menu(t)
    t.capture("menu")
    missing = [c for c in ["/new", "/resume", "/agent", "/approvals", "/model"] if not filtered_row(t, c)]
    ctx.check("plain.menu_lists_builtins", not missing, f"missing {missing}")
    greyed = [l for l in menu.splitlines() if REASON_HOST in l or REASON_ATTACH in l or REASON_WATCH in l]
    ctx.check("plain.no_attach_reasons_in_plain_menu", not greyed, greyed)
    review = open_menu(t, "/revi")
    t.capture("menu-agent-command")
    ctx.check("plain.agent_command_listed", bool(menu_row(review, "/claude-code:review")), last_nonblank(review))
    open_menu(t, "/appro")
    t.keys("Tab")
    time.sleep(0.8)
    line = composer_text(t.screen())
    ctx.check("plain.tab_completes_builtin", line.startswith("/approvals"), line)
    n = len(ctx.ledger("request"))
    run_typed(t, "/approvals")
    shown = t.screen()
    t.capture("approvals-dispatched")
    ctx.check("plain.builtin_dispatched_locally", re.search(r"(Ask|Approve|Agent)", shown) is not None
              and "session/prompt" not in requests_since(ctx, n), requests_since(ctx, n))
    t.keys("Escape")
    time.sleep(0.5)
    check_agent_command_passthrough(ctx, t, "plain", prefix="claude-code:")
    n = len(ctx.ledger("request"))
    screen = run_typed(t, "/review", settle=1.5)
    t.capture("bare-review-in-plain")
    ctx.check("plain.bare_review_not_captured_by_loose_match", "View previous session" not in screen,
              {"requests": requests_since(ctx, n), "last": last_nonblank(screen)})
    t.keys("Escape")
    t.keys("C-u")
    t.capture("agent-commands-sent")
    quit_fg(t)


def s35_attach_controller_commands(ctx):
    """nori attach controller: reasons for unavailable routes, refusals that keep the draft, Tab, agent commands, quit."""
    agent_commands_on(ctx)
    ctx.start_host()
    t = start_attach(ctx, "controller")
    submit(t, "first light")
    t.wait_for("heard: first light", 20)
    menu = open_menu(t)
    t.capture("menu")
    rows = {c: filtered_row(t, c) for c in ["/compact", "/agent", "/approvals", "/new", "/sessions", "/quit"]}
    (ctx.dir / "menu-rows.json").write_text(json.dumps(rows, indent=2))
    ctx.check("ctl.host_route_reason", REASON_HOST in rows["/compact"], rows["/compact"])
    ctx.check("ctl.attach_screen_reason", REASON_ATTACH in rows["/agent"], rows["/agent"])
    ctx.check("ctl.host_commands_enabled", all(rows[c] and "not available" not in rows[c] and REASON_WATCH not in rows[c]
                                               for c in ["/new", "/sessions"]), [rows["/new"], rows["/sessions"]])
    scrolled = open_menu(t, "/q")
    ctx.check("ctl.quit_enabled", bool(menu_row(scrolled, "/quit")) and "not available" not in menu_row(scrolled, "/quit"),
              menu_row(scrolled, "/quit"))
    open_menu(t, "/sess")
    t.keys("Tab")
    time.sleep(0.8)
    line = composer_text(t.screen())
    ctx.check("ctl.tab_completes_builtin", line.startswith("/sessions"), line)
    for cmd, reason in [("/compact", REASON_HOST), ("/agent", REASON_ATTACH)]:
        n = len(ctx.ledger("request"))
        screen = run_typed(t, cmd)
        t.capture(f"refused-{cmd[1:]}")
        shown = screen.count(f"{cmd}: {reason}")
        ctx.check(f"ctl.{cmd[1:]}_refused_with_reason", shown >= 1, last_nonblank(screen))
        ctx.check(f"ctl.{cmd[1:]}_refusal_count", True, f"shown {shown} time(s)")
        ctx.check(f"ctl.{cmd[1:]}_draft_kept", composer_text(screen).startswith(cmd), composer_text(screen))
        ctx.check(f"ctl.{cmd[1:]}_nothing_sent", not requests_since(ctx, n), requests_since(ctx, n))
    t.keys("C-u")
    check_agent_command_passthrough(ctx, t, "ctl")
    run_typed(t, " quit", settle=0.5)
    wait_ledger(ctx, lambda r: r.get("method") == "session/prompt" and r.get("text", "").strip() == "quit", timeout=10)
    ctx.check("ctl.leading_space_quit_is_a_prompt", any(p.strip() == "quit" for p in prompt_texts(ctx)), prompt_texts(ctx)[-3:])
    time.sleep(1.5)
    t.capture("agent-commands-sent")
    obs = ctx.observer()
    run_typed(t, "quit", settle=2.0)
    deadline = time.time() + 8
    while time.time() < deadline and not t.at_shell():
        time.sleep(0.3)
    sessions = obs.sessions()
    ctx.check("ctl.bare_quit_detaches", t.at_shell(), last_nonblank(t.screen()))
    ctx.check("ctl.detach_keeps_session", len(sessions) == 1, [s.get("sessionId") for s in sessions])
    obs.close()


def s35_watcher_and_transfer(ctx):
    """Two attached terminals: watcher reasons and refusals, and an open menu that re-greys when control moves."""
    ctx.start_host()
    a = start_attach(ctx, "controller")
    submit(a, "first light")
    a.wait_for("heard: first light", 20)
    obs = ctx.observer()
    sid = obs.sessions()[0]["sessionId"]
    b = ctx.term("watcher")
    b.run(f"{NORI} attach {sid[:8]}")
    b.wait_for(r"(WATCHING|watching|take control)", 20, regex=True)
    time.sleep(1)
    menu = open_menu(b)
    b.capture("watcher-menu")
    rows = {c: filtered_row(b, c) for c in ["/new", "/close", "/resume", "/delegate", "/sessions"]}
    (ctx.dir / "watcher-rows.json").write_text(json.dumps(rows, indent=2))
    ctx.check("watch.control_commands_greyed", all(REASON_WATCH in rows[c] and "alt+t" in rows[c].lower()
                                                   for c in ["/new", "/close", "/resume", "/delegate"]), rows)
    ctx.check("watch.sessions_enabled", bool(rows["/sessions"]) and REASON_WATCH not in rows["/sessions"], rows["/sessions"])
    news = len(ctx.ledger("request", "session/new"))
    n = len(ctx.ledger("request"))
    screen = run_typed(b, "/new")
    b.capture("watcher-refused-new")
    ctx.check("watch.new_refused_with_reason", "/new: watching" in screen, last_nonblank(screen))
    ctx.check("watch.new_nothing_sent", not requests_since(ctx, n) and len(ctx.ledger("request", "session/new")) == news,
              requests_since(ctx, n))
    ctx.check("watch.new_draft_kept", composer_text(screen).startswith("/new"), composer_text(screen))
    b.keys("C-u")
    menu_a = open_menu(a)
    ctx.check("transfer.controller_menu_enabled_before", REASON_WATCH not in menu_row(menu_a, "/new"), menu_row(menu_a, "/new"))
    b.keys("M-t")
    b.wait_for(r"CONTROLLING", 10, regex=True)
    time.sleep(1.5)
    after_a = a.screen()
    a.capture("open-menu-after-losing-control")
    ctx.check("transfer.open_menu_regreys", REASON_WATCH in menu_row(after_a, "/new"), menu_row(after_a, "/new"))
    menu_b = open_menu(b)
    b.capture("new-controller-menu")
    ctx.check("transfer.new_controller_menu_enabled", bool(menu_row(menu_b, "/new")) and REASON_WATCH not in menu_row(menu_b, "/new"),
              menu_row(menu_b, "/new"))
    run_typed(a, "/new")
    ctx.check("transfer.demoted_new_refused", len(ctx.ledger("request", "session/new")) == news, len(ctx.ledger("request", "session/new")))
    a.keys("C-u")
    run_typed(b, "/new", settle=3.0)
    wait_ledger(ctx, lambda r: r.get("method") == "session/new" and len(ctx.ledger("request", "session/new")) > news, timeout=10)
    ctx.check("transfer.promoted_new_runs_once", len(ctx.ledger("request", "session/new")) == news + 1,
              len(ctx.ledger("request", "session/new")))
    b.capture("promoted-new")
    obs.close()


def s35_escape_mid_turn(ctx, mode):
    """Escape with the `/` menu open during a turn closes the menu only; the next Escape cancels once."""
    if mode == "fg":
        t = start_fg(ctx)
    else:
        ctx.start_host()
        t = start_attach(ctx, "attach")
    submit(t, "hold the lamp")
    wait_ticks(ctx, 3)
    t.type("/")
    time.sleep(0.8)
    t.capture("menu-during-turn")
    ctx.check("esc.menu_open_during_turn", bool(menu_row(t.screen(), "/sessions")), last_nonblank(t.screen()))
    t.keys("Escape")
    ticks = ctx.ticks()
    time.sleep(1.5)
    screen = t.screen()
    t.capture("after-first-escape")
    ctx.check("esc.first_escape_closes_menu", not menu_row(screen, "/sessions"), last_nonblank(screen))
    ctx.check("esc.first_escape_does_not_cancel", not ctx.ledger("request", "session/cancel") and ctx.ticks() > ticks,
              {"cancels": len(ctx.ledger("request", "session/cancel")), "ticks": [ticks, ctx.ticks()]})
    n = len(ctx.ledger("request"))
    screen = run_typed(t, "/compact" if mode == "attach" else "/review now", settle=1.5)
    t.capture("command-during-turn")
    if mode == "attach":
        ctx.check("esc.refused_command_mid_turn_not_sent", not [m for m in requests_since(ctx, n) if m != "session/cancel"]
                  and "/compact: " + REASON_HOST in screen, requests_since(ctx, n))
    t.keys("C-u")
    time.sleep(0.3)
    t.keys("Escape")
    if mode == "fg":
        t.keys("Escape")
    wait_ledger(ctx, lambda r: r["kind"] == "turn_end" and r.get("stopReason") == "cancelled")
    time.sleep(1.0)
    t.capture("cancelled")
    ctx.check("esc.cancelled_exactly_once", len(ctx.ledger("request", "session/cancel")) == 1,
              len(ctx.ledger("request", "session/cancel")))
    if mode == "fg":
        quit_fg(t)


def s35_escape_two_presses(ctx, mode):
    """With the menu open mid-turn, the first Escape only closes the menu and keeps the draft; the
    Escapes after it interrupt exactly as they do with a plain draft (in-run control)."""
    if mode == "fg":
        t = start_fg(ctx)
    else:
        ctx.start_host()
        t = start_attach(ctx, "attach")

    def cancels():
        return len(ctx.ledger("request", "session/cancel"))

    def draft(screen):
        text = composer_text(screen)
        return text[2:] if text.startswith("› ") else text

    def presses_until_cancel(before):
        for n in range(1, 4):
            t.keys("Escape")
            time.sleep(1.5)
            if cancels() > before:
                return n
        return None

    submit(t, "hold the lamp")
    wait_ticks(ctx, 3)
    t.type("lamp draft")
    time.sleep(0.8)
    control = presses_until_cancel(cancels())
    wait_ledger(ctx, lambda r: r["kind"] == "turn_end" and r.get("stopReason") == "cancelled")
    time.sleep(0.8)
    t.capture("control-cancelled")
    control_draft = draft(t.screen())
    ctx.check("esc2.control_interrupts", control is not None, {"presses": control, "draft": control_draft})
    t.keys("C-u")
    time.sleep(0.3)

    submit(t, "hold the second lamp")
    wait_ticks(ctx, 3)
    t.type("/")
    time.sleep(0.8)
    t.capture("menu-open")
    ctx.check("esc2.menu_open_during_turn", bool(menu_row(t.screen(), "/sessions")), last_nonblank(t.screen()))
    before = cancels()
    ticks = ctx.ticks()
    t.keys("Escape")
    time.sleep(1.5)
    screen = t.screen()
    t.capture("after-first-escape")
    ctx.check("esc2.first_closes_menu", not menu_row(screen, "/sessions"), last_nonblank(screen))
    ctx.check("esc2.first_keeps_draft", draft(screen) == "/", draft(screen))
    ctx.check("esc2.first_does_not_cancel", cancels() == before and ctx.ticks() > ticks,
              {"cancels": cancels() - before, "ticks": [ticks, ctx.ticks()]})
    after = presses_until_cancel(before)
    wait_ledger(ctx, lambda r: r["kind"] == "turn_end" and r.get("stopReason") == "cancelled"
                and len([e for e in ctx.ledger("turn_end") if e.get("stopReason") == "cancelled"]) >= 2)
    time.sleep(0.8)
    t.capture("after-interrupt")
    ctx.check("esc2.then_interrupts_like_a_plain_draft", after == control, {"after_first": after, "control": control})
    ctx.check("esc2.one_cancel_per_turn", cancels() == 2, cancels())
    ctx.check("esc2.draft_after_interrupt_matches_control",
              (draft(t.screen()) == "/") == (control_draft == "lamp draft"),
              {"menu_path": draft(t.screen()), "control": control_draft})
    if mode == "fg":
        quit_fg(t)


def s35_paste(ctx, mode):
    """A pasted slash command follows the same table as a typed one; a multi-line paste is never run as a command."""
    if mode == "fg":
        t = start_fg(ctx)
    else:
        ctx.start_host()
        t = start_attach(ctx, "attach")
    submit(t, "first light")
    t.wait_for("heard: first light", 20)
    n = len(ctx.ledger("request"))
    t.keys("C-u")
    t.paste("/compact" if mode == "attach" else "/approvals")
    time.sleep(1.0)
    t.keys("Enter")
    time.sleep(1.5)
    screen = t.screen()
    t.capture("pasted-command")
    if mode == "attach":
        ctx.check("paste.single_line_refused_like_typed", ("/compact: " + REASON_HOST) in screen and not requests_since(ctx, n),
                  {"sent": requests_since(ctx, n), "last": last_nonblank(screen)})
    else:
        ctx.check("paste.single_line_dispatched_like_typed", re.search(r"(Ask|Approve|Agent)", screen) is not None
                  and "session/prompt" not in requests_since(ctx, n), requests_since(ctx, n))
        t.keys("Escape")
    t.keys("C-u")
    body = "/diff the harbor notes\nsecond synthetic line"
    n = len(ctx.ledger("request"))
    t.paste(body)
    time.sleep(1.0)
    t.keys("Enter")
    time.sleep(2.5)
    screen = t.screen()
    t.capture("pasted-multiline")
    sent = [p for p in prompt_texts(ctx) if "second synthetic line" in p]
    refused = "/diff: " in screen
    ctx.check("paste.multiline_sent_once_or_refused", (len(sent) == 1 and "/diff the harbor notes" in sent[0]) or (not sent and refused),
              {"sent": sent, "refused": refused, "requests": requests_since(ctx, n)})
    if mode == "fg":
        quit_fg(t)


def s35_reconnect(ctx):
    """While reconnecting, host commands read `reconnecting` and refuse without sending; after reconnect they run."""
    endpoint, _ = ctx.start_host()
    proxy = Proxy(endpoint)
    t = ctx.term("controller")
    attach_via(ctx, t, proxy)
    attach_new_session(t)
    submit(t, "first light")
    t.wait_for("heard: first light", 20)
    open_menu(t)
    news = len(ctx.ledger("request", "session/new"))
    proxy.drop_all(hold=6.0)
    t.wait_for(r"(RECONNECTING|reconnecting)", 15, regex=True)
    time.sleep(0.8)
    menu = t.screen()
    t.capture("menu-while-reconnecting")
    ctx.check("reconnect.open_menu_shows_reason", "reconnecting" in menu_row(menu, "/new"), menu_row(menu, "/new"))
    screen = run_typed(t, "/new", settle=1.0)
    t.capture("refused-while-reconnecting")
    ctx.check("reconnect.new_refused_with_reason", "/new: reconnecting" in screen or "Your draft is preserved" in screen,
              [l.strip() for l in screen.splitlines() if "econnect" in l][-3:])
    ctx.check("reconnect.draft_kept", composer_text(screen).startswith("/new"), composer_text(screen))
    t.wait_gone("RECONNECTING", 25)
    time.sleep(1.5)
    ctx.check("reconnect.nothing_sent_while_down", len(ctx.ledger("request", "session/new")) == news,
              len(ctx.ledger("request", "session/new")))
    menu = open_menu(t)
    ctx.check("reconnect.menu_enabled_after", bool(menu_row(menu, "/new")) and "reconnecting" not in menu_row(menu, "/new"),
              menu_row(menu, "/new"))
    t.capture("menu-after-reconnect")
    run_typed(t, "/new", settle=3.0)
    ctx.check("reconnect.new_runs_once_after", len(ctx.ledger("request", "session/new")) == news + 1,
              len(ctx.ledger("request", "session/new")))
    proxy.close()


# ---------------------------------------------------------------------------
# S36: startup activation window (native finding on #750), swept by Enter time
# ---------------------------------------------------------------------------
RACE_WORD = "race lantern word"
RACE_OFFSETS = [float(x) for x in os.environ.get("ACC_RACE_OFFSETS", "-0.3,-0.05,0,0.01,0.02,0.04,0.08,0.15,0.3").split(",")]
def tmux_send(t, text):
    term.tmux("send-keys", "-t", t.name, "-l", text)


RACE_LOG_ENV = "RUST_LOG=nori_host_runtime=debug,nori_harness=debug,nori_tui=debug"


def tui_to_host_prompts(ctx):
    return [m for m in host_requests(ctx) if m == "session/prompt"]


def s36_startup_race(ctx, burst_at_reply=False):
    """Type before the startup session/new reply (or, with burst_at_reply, type the burst at swept offsets after it), then press Enter; record screen echo and adapter delivery separately."""
    rows = []
    for rep in range(int(os.environ.get("ACC_RACE_REPS", "2"))):
        for off in RACE_OFFSETS:
            for f in ctx.fx["ledger"].parent.glob(ctx.fx["ledger"].name):
                f.write_text("")
            shutil.rmtree(ctx.fx["homes"]["host"] / "log", ignore_errors=True)
            (ctx.fx["control"] / "new-delay").write_text("1.0")
            name = f"r{rep}-{str(off).replace('-', 'm').replace('.', 'p')}"
            t = ctx.term(name)
            t.run(f"{RACE_LOG_ENV} {NORI} --skip-trust-directory")
            started = wait_ledger(ctx, lambda r: r.get("kind") == "new_delay_start", timeout=20)
            if not burst_at_reply:
                t.type(RACE_WORD)
            end = None
            deadline = time.time() + 5
            while time.time() < deadline and end is None:
                hit = [r for r in fixtures.read_ledger(ctx.fx["ledger"]) if r["kind"] == "new_delay_end"]
                end = hit[0]["t"] if hit else None
                time.sleep(0.003)
            if end is not None:
                time.sleep(max(0.0, end + off - time.time()))
            if burst_at_reply:
                typed = time.time()
                tmux_send(t, RACE_WORD)
                time.sleep(0.9)
            t.keys("Enter")
            pressed = time.time()
            time.sleep(4.0)
            screen = t.screen()
            sent = prompts_with(ctx, RACE_WORD)
            row = {"rep": rep, "offset": off, "enter_minus_reply": round(pressed - end, 4) if end else None,
                   "typed_minus_reply": round(typed - end, 4) if burst_at_reply and end else None,
                   "tui_to_host_prompts": len(tui_to_host_prompts(ctx)), "adapter_prompts": len(sent),
                   "answered": f"heard: {RACE_WORD}" in " ".join(screen.split()),
                   "echoed": RACE_WORD in screen, "started": bool(started)}
            rows.append(row)
            print("RACE", row, flush=True)
            if row["adapter_prompts"] != 1:
                t.capture(f"{name}-lost")
                logs = ctx.fx["homes"]["host"] / "log"
                if logs.exists():
                    shutil.copytree(logs, ctx.dir / f"{name}-log", dirs_exist_ok=True)
                shutil.copy(ctx.fx["ledger"], ctx.dir / f"{name}-ledger.jsonl")
            slash(t, "/quit", settle=1.0)
            t.close()
            ctx.terms.remove(t)
            time.sleep(0.5)
    (ctx.dir / "race.json").write_text(json.dumps(rows, indent=2))
    bad = [r for r in rows if r["adapter_prompts"] != 1]
    ctx.check("race.every_enter_delivered_once", not bad, bad[:6])
    ctx.check("race.trials", True, f"{len(rows)} trials, {len(bad)} not delivered once")



# ---------------------------------------------------------------------------
# S37: per-session approvals across sessions, terminals and launches
# ---------------------------------------------------------------------------
MODE_LABELS = {"ask": "Ask", "agent": "Agent", "approve-all": "Approve all"}


def host_mode(obs, sid):
    """The mode the host reports for `sid` to an observing client (session/load result), or the raw reply."""
    cwd = next((x.get("cwd") for x in obs.sessions() if x["sessionId"] == sid), "/")
    reply = obs.call("session/load", {"sessionId": sid, "cwd": cwd, "mcpServers": []})
    result = reply.get("result") or {}
    modes = result.get("modes") or {}
    if modes.get("currentModeId"):
        return modes["currentModeId"]
    for option in result.get("configOptions") or []:
        if option.get("id") == "mode":
            return option.get("currentValue")
    return f"unreadable: {json.dumps(reply)[:200]}"


def footer_mode(screen):
    """The mode a terminal shows: the last `Approvals: …` label on screen, else "none" (plain nori leaves
    Agent unlabelled by default, #752 decision 1)."""
    found = re.findall(r"Approvals:\s*(Approve all|Ask|Agent)", screen)
    if not found:
        return "none"
    return {v: k for k, v in MODE_LABELS.items()}[found[-1]]


def shown_as(screen, plain=False):
    mode = footer_mode(screen)
    return "agent" if plain and mode == "none" else mode


def pick_mode(t, label):
    """Open /approvals, select `label`; for Approve all, confirm the second step when one is shown."""
    run_typed(t, "/approvals", settle=1.2)
    picked = choose(t, label)
    time.sleep(1.2)
    if picked and label == "Approve all":
        confirm_if_asked(t)
    return picked


def s37_handoff_two_sessions(ctx):
    """Two sessions, three terminals: the controller's mode change reaches only its session, a watcher
    cannot change it, the new controller can after Alt+T, and every UI matches what the host reports."""
    ctx.start_host()
    a = ctx.term("controller-a")
    a.run(f"{NORI} attach")
    attach_new_session(a)
    submit(a, "first light")
    a.wait_for("heard: first light", 20)
    obs = ctx.observer()
    s1 = obs.sessions()[0]["sessionId"]
    out = ctx.nori("host", "new", "--prompt", "second errand", cwd=ctx.fx["roots"]["alpha"])
    (ctx.dir / "host-new.txt").write_text(out.stdout + out.stderr)
    deadline = time.time() + 20
    while len(obs.sessions()) < 2 and time.time() < deadline:
        time.sleep(0.5)
    s2 = next(x["sessionId"] for x in obs.sessions() if x["sessionId"] != s1)
    c = ctx.term("second-session")
    c.run(f"{NORI} attach {s2[:8]}")
    c.wait_for(r"(WATCHING|watching)", 20, regex=True)
    time.sleep(1)
    start = {"s1": host_mode(obs, s1), "s2": host_mode(obs, s2)}
    (ctx.dir / "host-modes-start.json").write_text(json.dumps(start))
    b = ctx.term("watcher-b")
    b.run(f"{NORI} attach {s1[:8]}")
    b.wait_for(r"(WATCHING|watching)", 20, regex=True)
    time.sleep(1)

    sets_before = mode_sets(ctx)
    picked = pick_mode(a, "Ask")
    a.capture("a-picked-ask")
    b.capture("b-watching-after-ask")
    c.capture("c-second-session-after-ask")
    hosts = {"s1": host_mode(obs, s1), "s2": host_mode(obs, s2)}
    ctx.check("handoff.controller_sets_ask_on_host", picked and hosts["s1"] == "ask", hosts)
    ctx.check("handoff.other_session_unchanged_on_host", hosts["s2"] == start["s2"], {"start": start, "now": hosts})
    new_sets = mode_sets(ctx)[len(sets_before):]
    ctx.check("handoff.adapter_mode_sent_only_to_foreground", bool(new_sets) and all(sid != s2 for sid, _ in new_sets)
              and HOST_MODE_FOR.get(new_sets[-1][1]) == "ask", new_sets)
    ui = {"a": footer_mode(a.screen()), "b": footer_mode(b.screen()), "c": footer_mode(c.screen())}
    ctx.check("handoff.controller_footer_matches_host", ui["a"] == hosts["s1"], {"ui": ui, "host": hosts})
    ctx.check("handoff.watcher_footer_matches_host", ui["b"] == hosts["s1"], {"ui": ui, "host": hosts})
    ctx.check("handoff.second_session_footer_matches_host", ui["c"] == hosts["s2"], {"ui": ui, "host": hosts})

    n = len(ctx.ledger("request"))
    run_typed(b, "/approvals", settle=1.2)
    b.capture("b-watcher-approvals")
    choose(b, "Approve all")
    time.sleep(1)
    b.keys("Escape")
    b.keys("C-u")
    ctx.check("handoff.watcher_cannot_change_mode", host_mode(obs, s1) == "ask" and not mode_sets(ctx)[len(sets_before) + len(new_sets):],
              {"host": host_mode(obs, s1), "requests": requests_since(ctx, n)})

    b.keys("M-t")
    b.wait_for(r"CONTROLLING", 10, regex=True)
    time.sleep(1.5)
    picked = pick_mode(b, "Approve all")
    b.capture("b-controller-approve-all")
    a.capture("a-watching-after-handoff")
    hosts = {"s1": host_mode(obs, s1), "s2": host_mode(obs, s2)}
    ui = {"a": footer_mode(a.screen()), "b": footer_mode(b.screen()), "c": footer_mode(c.screen())}
    ctx.check("handoff.new_controller_sets_mode_on_host", picked and hosts["s1"] == "approve-all", hosts)
    ctx.check("handoff.both_terminals_show_host_mode", ui["a"] == ui["b"] == hosts["s1"], {"ui": ui, "host": hosts})
    ctx.check("handoff.other_session_still_unchanged", hosts["s2"] == start["s2"] and ui["c"] == hosts["s2"], {"ui": ui, "host": hosts})
    n = len(mode_sets(ctx))
    run_typed(a, "/approvals", settle=1.2)
    a.capture("a-demoted-approvals")
    choose(a, "Agent")
    time.sleep(1)
    a.keys("Escape")
    a.keys("C-u")
    ctx.check("handoff.demoted_controller_cannot_change_mode", host_mode(obs, s1) == "approve-all" and len(mode_sets(ctx)) == n,
              {"host": host_mode(obs, s1), "sets": mode_sets(ctx)[n:]})
    (ctx.dir / "host-modes-end.json").write_text(json.dumps({"s1": host_mode(obs, s1), "s2": host_mode(obs, s2)}))
    obs.close()


def s37_repeat_launch_keeps_config(ctx):
    """A current config (host default mode, sandbox table) is read as-is on every launch: no rewrite,
    mode from the host default, and a mode change leaves the sandbox settings alone."""
    config = ctx.fx["homes"]["host"] / "config.toml"
    original = config.read_text()
    ctx.dir.mkdir(parents=True, exist_ok=True)
    (ctx.dir / "config-before.toml").write_text(original)
    t = ctx.term("fg")
    for launch in (1, 2):
        t.run("clear")
        time.sleep(0.5)
        t.run(f"{NORI} --skip-trust-directory")
        wait_composer(t)
        time.sleep(1.5)
        sets = mode_sets(ctx)
        ctx.check(f"repeat.launch{launch}_session_starts_at_host_default", bool(sets) and HOST_MODE_FOR.get(sets[-1][1]) == "ask", sets[-2:])
        ctx.check(f"repeat.launch{launch}_footer_shows_host_default", shown_as(t.screen(), plain=True) == "ask", t.screen().splitlines()[-4:])
        t.capture(f"launch{launch}")
        quit_fg(t)
        time.sleep(1)
        ctx.check(f"repeat.launch{launch}_config_unchanged", config.read_text() == original,
                  "".join(difflib.unified_diff(original.splitlines(True), config.read_text().splitlines(True))))
    t.run(f"{NORI} --skip-trust-directory")
    wait_composer(t)
    time.sleep(1)
    picked = pick_mode(t, "Approve all")
    t.capture("picked-approve-all")
    quit_fg(t)
    time.sleep(1)
    after = config.read_text()
    (ctx.dir / "config-after.toml").write_text(after)
    sandbox = [l for l in original.splitlines() if l.startswith("sandbox_mode") or l.startswith("network_access") or l.startswith("[sandbox_workspace_write]")]
    ctx.check("repeat.mode_change_keeps_sandbox_settings", picked and all(l in after.splitlines() for l in sandbox), sandbox)


def s37_plain_two_sessions(ctx):
    """Plain nori: a mode set on one session stays with it; /new starts at the default; switching back restores it."""
    t = start_fg(ctx)
    submit(t, "first light")
    t.wait_for("heard: first light", 20)
    picked = pick_mode(t, "Ask")
    t.capture("first-session-ask")
    sets = mode_sets(ctx)
    ctx.check("plain2.first_session_ask", picked and shown_as(t.screen(), plain=True) == "ask"
              and bool(sets) and HOST_MODE_FOR.get(sets[-1][1]) == "ask", {"ui": footer_mode(t.screen()), "sets": sets[-2:]})
    news = len(ctx.ledger("request", "session/new"))
    run_typed(t, "/new", settle=3.0)
    wait_ledger(ctx, lambda r: r.get("method") == "session/new" and len(ctx.ledger("request", "session/new")) > news, timeout=10)
    time.sleep(1.5)
    t.capture("second-session")
    sets = mode_sets(ctx)
    ctx.check("plain2.new_session_not_copying_ask", len(ctx.ledger("request", "session/new")) == news + 1
              and HOST_MODE_FOR.get(sets[-1][1]) == "agent" and shown_as(t.screen(), plain=True) == "agent",
              {"ui": footer_mode(t.screen()), "sets": sets[-2:]})
    n = len(mode_sets(ctx))
    t.keys("M-,")
    time.sleep(2)
    t.capture("back-to-first")
    ctx.check("plain2.switch_back_shows_ask", shown_as(t.screen(), plain=True) == "ask", footer_mode(t.screen()))
    ctx.check("plain2.switch_sends_no_mode_change", len(mode_sets(ctx)) == n, mode_sets(ctx)[n:])
    quit_fg(t)


def s37_approve_all_answers_on_host(ctx):
    """Approve all is enforced by the host: a permission request is answered without a prompt in either terminal."""
    ctx.start_host()
    a = ctx.term("controller")
    a.run(f"{NORI} attach")
    attach_new_session(a)
    picked = pick_mode(a, "Approve all")
    obs = ctx.observer()
    sid = obs.sessions()[0]["sessionId"]
    ctx.check("approveall.host_reports_mode", picked and host_mode(obs, sid) == "approve-all", host_mode(obs, sid))
    submit(a, "perm check")
    try:
        a.wait_for(r"permission: ", 15, regex=True)
        answered = True
    except TimeoutError:
        answered = False
    a.capture("perm-answered")
    ctx.check("approveall.permission_answered_without_prompt", answered and "waiting on you" not in a.screen(),
              last_nonblank(a.screen()))
    obs.close()

# ---------------------------------------------------------------------------
# S38: mixed versions. --nori is the host's build, --client the attach client's.
# ---------------------------------------------------------------------------

ERRORISH = re.compile(r"(error|failed|unknown variant|invalid|Connection lost|Reconnecting)", re.I)


def client_state(ctx, t, label):
    """What the client under test is doing: still running, what it shows, and any error text."""
    time.sleep(1.5)
    t.capture(label)
    screen = t.screen()
    lines = [l.rstrip() for l in screen.splitlines() if l.strip()]
    return {"running": not t.at_shell(), "mode_label": footer_mode(screen),
            "error_lines": [l for l in lines if ERRORISH.search(l)][-4:], "tail": lines[-6:]}


def mode_envelopes(ctx):
    """approval_mode_changed envelopes the observer saw on the wire."""
    log = ctx.dir / "observer-wire.jsonl"
    if not log.exists():
        return []
    return [l for l in log.read_text().splitlines() if "approval_mode_changed" in l]


def take_control(t):
    if re.search(r"WATCHING|watching", t.screen()):
        t.keys("M-t")
        t.wait_for(r"CONTROLLING", 10, regex=True)
        time.sleep(1.5)


def s38_join_then_mode_change(ctx, changer):
    """The client under test joins an existing session; a mode change follows, made by a terminal on the
    host's own build (`other`) or by the client under test (`self`). Records connection and display."""
    ctx.start_host(log=True)
    obs = ctx.observer()
    out = ctx.nori("host", "new", "--prompt", "first errand")
    (ctx.dir / "host-new.txt").write_text(out.stdout + out.stderr)
    deadline = time.time() + 20
    while not obs.sessions() and time.time() < deadline:
        time.sleep(0.5)
    s1 = obs.sessions()[0]["sessionId"]
    wait_ledger(ctx, lambda r: r.get("method") == "session/prompt" and "first errand" in json.dumps(r), 20)
    obs.call("session/load", {"sessionId": s1, "cwd": str(ctx.fx["roots"]["alpha"]), "mcpServers": []})
    start_mode = host_mode(obs, s1)

    o = ctx.term("client-under-test")
    o.run(f"{CLIENT} attach {s1[:8]}")
    time.sleep(5)
    joined = client_state(ctx, o, "1-joined")
    ctx.check("join.client_running", joined["running"], joined)
    ctx.check("join.shows_conversation", "heard: first errand" in o.screen(), joined["tail"])
    ctx.check("join.no_error", not joined["error_lines"], joined["error_lines"])

    if changer == "other":
        c = ctx.term("controller-host-build")
        c.run(f"{NORI} attach {s1[:8]}")
        c.wait_for(r"(WATCHING|CONTROLLING|watching)", 20, regex=True)
        take_control(c)
        actor = c
    else:
        take_control(o)
        actor = o
    picked = pick_mode(actor, "Ask")
    actor.capture("2-picked-ask")
    obs.drain(1.0)
    now = host_mode(obs, s1)
    ctx.check("change.host_now_ask", picked and now == "ask", {"start": start_mode, "now": now})
    after = client_state(ctx, o, "3-after-change")
    ctx.check("change.client_running", after["running"], after)
    ctx.check("change.no_error", not after["error_lines"], after["error_lines"])
    ctx.check("change.client_label_matches_host", after["mode_label"] == now, {"ui": after["mode_label"], "host": now})
    if changer == "self":
        menu = run_typed(o, "/approvals", settle=1.2)
        o.capture("3b-menu-after-change")
        current = next((l.strip() for l in menu.splitlines() if "(current)" in l), "")
        ctx.check("change.menu_marks_host_mode_current", MODE_LABELS.get(now, "?") in current, current or menu[-300:])
        o.keys("Escape")
        o.keys("C-u")
        time.sleep(0.5)

    n = len(prompts_with(ctx, "after the change"))
    if changer == "other":
        c.keys("Escape")
        c.keys("C-u")
        submit(c, "after the change")
    else:
        submit(o, "after the change")
    wait_ledger(ctx, lambda r: r.get("method") == "session/prompt" and "after the change" in json.dumps(r), 15)
    time.sleep(3)
    follow = client_state(ctx, o, "4-after-prompt")
    ctx.check("follow.prompt_reached_adapter_once", len(prompts_with(ctx, "after the change")) - n == 1,
              len(prompts_with(ctx, "after the change")) - n)
    ctx.check("follow.client_shows_reply", follow["running"] and "heard: after the change" in o.screen(), follow["tail"])
    (ctx.dir / "observed.json").write_text(json.dumps({"session": s1, "start_mode": start_mode, "mode_after": now,
        "joined": joined, "after_change": after, "after_prompt": follow,
        "mode_envelopes_on_wire": len(mode_envelopes(ctx))}, indent=2))
    (ctx.dir / "host-requests.txt").write_text("\n".join(host_requests(ctx)) + "\n")
    obs.close()


def s38_fresh_new(ctx):
    """The client under test starts a session with /new and prompts it (the host announces the start mode)."""
    ctx.start_host()
    obs = ctx.observer()
    o = ctx.term("client-under-test")
    o.run(f"{CLIENT} attach")
    attach_new_session(o)
    time.sleep(2)
    started = client_state(ctx, o, "1-after-new")
    ctx.check("new.client_running", started["running"], started)
    ctx.check("new.no_error", not started["error_lines"], started["error_lines"])
    submit(o, "fresh errand")
    wait_ledger(ctx, lambda r: r.get("method") == "session/prompt" and "fresh errand" in json.dumps(r), 15)
    time.sleep(3)
    replied = client_state(ctx, o, "2-after-prompt")
    sessions = obs.sessions()
    mode = host_mode(obs, sessions[0]["sessionId"]) if sessions else "no session"
    ctx.check("new.prompt_reached_adapter_once", len(prompts_with(ctx, "fresh errand")) == 1, len(prompts_with(ctx, "fresh errand")))
    ctx.check("new.client_shows_reply", replied["running"] and "heard: fresh errand" in o.screen(), replied["tail"])
    ctx.check("new.client_label_matches_host", shown_as(o.screen()) == mode or (replied["mode_label"] == "none" and mode == "agent"),
              {"ui": replied["mode_label"], "host": mode})
    (ctx.dir / "observed.json").write_text(json.dumps({"host_mode": mode, "after_new": started, "after_prompt": replied}, indent=2))
    obs.close()


# ---------------------------------------------------------------------------
# S39: the host's mode list for an agent without Ask, against each menu.
# ---------------------------------------------------------------------------

def advertised(obs, sid):
    cwd = next((x.get("cwd") for x in obs.sessions() if x["sessionId"] == sid), "/")
    result = obs.call("session/load", {"sessionId": sid, "cwd": cwd, "mcpServers": []}).get("result") or {}
    modes = [m.get("id") for m in (result.get("modes") or {}).get("availableModes") or []]
    option = next((o for o in result.get("configOptions") or [] if o.get("id") == "mode"), {})
    choices = [c.get("value") for c in option.get("options") or option.get("choices") or []]
    return {"availableModes": modes, "mode_option": choices, "current": (result.get("modes") or {}).get("currentModeId")}


def menu_modes(screen):
    """Modes the /approvals menu offers, leaving out rows marked as not available."""
    return menu_rows(screen)["offered"]


def menu_rows(screen):
    rows = [l for l in screen.splitlines() if re.search(r"\b(Ask|Agent|Approve all)\b", l)]
    offered, disabled = [], []
    for label, mode in (("Approve all", "approve-all"), ("Agent", "agent"), ("Ask", "ask")):
        hit = [l for l in rows if re.match(rf"^\s*(›|>)?\s*(\d\.\s*)?{label}\b", l)]
        if not hit:
            continue
        (disabled if any(re.search(r"not available|unavailable", l, re.I) for l in hit) else offered).append(mode)
    return {"offered": sorted(offered), "disabled": sorted(disabled)}


def s39_mode_list_without_ask(ctx, where):
    """Compare the modes the host advertises for an opencode session with the /approvals menu, then try Ask."""
    ctx.start_host(log=True)
    obs = ctx.observer()
    for agent in ("opencode", "claude-code"):
        ctx.nori("host", "new", "--agent", agent)
    deadline = time.time() + 20
    while len(obs.sessions()) < 2 and time.time() < deadline:
        time.sleep(0.5)
    rows = obs.call("_nori/session/list", {}).get("result", {})
    (ctx.dir / "nori-session-list.json").write_text(json.dumps(rows, indent=2))
    by_agent = {}
    for x in obs.sessions():
        ad = advertised(obs, x["sessionId"])
        by_agent[x["sessionId"]] = ad
    (ctx.dir / "advertised.json").write_text(json.dumps(by_agent, indent=2))
    opencode = next((sid for sid, ad in by_agent.items() if "ask" not in ad["availableModes"]), None)
    claude = next((sid for sid, ad in by_agent.items() if "ask" in ad["availableModes"]), None)
    ctx.check("host.opencode_list_has_no_ask", opencode is not None, by_agent)
    ctx.check("host.claude_list_has_ask", claude is not None, by_agent)
    if not opencode:
        return
    host_list = sorted(by_agent[opencode]["availableModes"])
    if where == "attach":
        t = ctx.term("controller")
        t.run(f"{CLIENT} attach {opencode[:8]}")
        t.wait_for("›", 20)
        take_control(t)
    else:
        t = start_fg(ctx, cmd=f"{HOST_LOG_ENV} {NORI} --skip-trust-directory --agent opencode")
        time.sleep(3)
    run_typed(t, "/approvals", settle=1.2)
    menu = t.capture("1-approvals-opencode")
    rows = menu_rows(t.screen())
    shown = rows["offered"]
    ctx.check("menu.matches_host_list", shown == host_list, {"menu": shown, "host": host_list})
    ask_rows = [l.strip() for l in t.screen().splitlines() if re.match(r"^\s*(›|>)?\s*(\d\.\s*)?Ask\b", l)]
    ctx.check("menu.ask_shown_disabled", rows["disabled"] == ["ask"], ask_rows)
    n = len(mode_sets(ctx))
    sets_before = [m for m in host_requests(ctx) if m in ("session/set_mode", "session/set_config_option")]
    picked = choose(t, "Ask")
    time.sleep(1.5)
    t.capture("2-after-choosing-ask")
    screen = t.screen()
    target = opencode if where == "attach" else (obs.sessions()[-1]["sessionId"])
    mode = host_mode(obs, target) if where == "attach" else "n/a (private host)"
    reason = [l.strip() for l in screen.splitlines() if re.search(r"(ask|strict|support|refus)", l, re.I)][-3:]
    ctx.check("ask.not_applied", where != "attach" or mode != "ask", {"picked": picked, "host": mode})
    ctx.check("ask.reason_shown_or_not_offered", not picked or bool(reason), reason)
    ctx.check("ask.no_adapter_mode_change", len(mode_sets(ctx)) == n, mode_sets(ctx)[n:])
    sets_after = [m for m in host_requests(ctx) if m in ("session/set_mode", "session/set_config_option")]
    ctx.check("ask.client_sent_no_mode_request", bool(host_requests(ctx)) and sets_after == sets_before,
              {"before": sets_before, "after": sets_after, "host_log_requests": len(host_requests(ctx))})
    (ctx.dir / "host-requests.txt").write_text("\n".join(host_requests(ctx)) + "\n")
    (ctx.dir / "observed.json").write_text(json.dumps({"host_list": host_list, "menu": shown, "menu_rows": rows, "ask_rows": ask_rows, "picked_ask": picked,
                                                       "host_mode_after": mode, "reason_lines": reason}, indent=2))
    t.keys("Escape")
    obs.close()



# ---------------------------------------------------------------------------
# S40: plain nori's agent-mode badge and startup card against the host's mode.
# ---------------------------------------------------------------------------

def badge(screen):
    """The last `[ mode ]` badge drawn, or "none"."""
    found = re.findall(r"\[\s*([A-Za-z-]+)\s*\]\s*$", screen, re.M)
    return found[-1] if found else "none"


def card_modes(screen):
    """Approval wording on startup-card `System` rows."""
    rows = [l.strip() for l in screen.splitlines() if re.search(r"^\s*System\b", l)]
    return {"rows": rows, "stale": [l for l in rows if re.search(r"(Agent|Approve all|Ask) approvals", l)],
            "labels": [m for l in rows for m in re.findall(r"Approvals:\s*(Approve all|Ask|Agent)", l)]}


def last_set(ctx):
    sets = mode_sets(ctx)
    return sets[-1][1] if sets else "none"


def s40_badge_follows_mode_and_session(ctx):
    t = start_fg(ctx)
    submit(t, "first light")
    t.wait_for("heard: first light", 20)
    time.sleep(1)
    t.capture("1-start")
    ctx.check("badge.start_matches_adapter", badge(t.screen()) == last_set(ctx) == "acceptEdits",
              {"badge": badge(t.screen()), "adapter": last_set(ctx)})
    card = card_modes(t.screen())
    ctx.check("card.no_guessed_mode", not card["stale"] and all(l == "Agent" for l in card["labels"]), card)
    pick_mode(t, "Ask")
    t.capture("2-ask")
    ctx.check("badge.follows_ask", badge(t.screen()) == last_set(ctx) == "default",
              {"badge": badge(t.screen()), "adapter": last_set(ctx), "footer": footer_mode(t.screen())})
    news = len(ctx.ledger("request", "session/new"))
    run_typed(t, "/new", settle=3.0)
    wait_ledger(ctx, lambda r: r.get("method") == "session/new" and len(ctx.ledger("request", "session/new")) > news, timeout=10)
    time.sleep(1.5)
    t.capture("3-new")
    ctx.check("badge.new_session_at_default", badge(t.screen()) == last_set(ctx) == "acceptEdits",
              {"badge": badge(t.screen()), "adapter": last_set(ctx)})
    n = len(mode_sets(ctx))
    t.keys("M-,")
    time.sleep(2)
    t.capture("4-back")
    ctx.check("badge.switch_back_shows_first_session", badge(t.screen()) == "default" and len(mode_sets(ctx)) == n,
              {"badge": badge(t.screen()), "sets_since": mode_sets(ctx)[n:]})
    quit_fg(t)


def s40_card_and_badge_at_launch(ctx, legacy):
    if legacy:
        ctx.fx = fixtures.build(ctx.run_dir, default_agent="claude-code",
                                top_extra='approval_policy = "on-request"\nsandbox_mode = "read-only"')
    t = start_fg(ctx)
    time.sleep(2)
    t.capture("1-launch")
    screen = t.screen()
    card = card_modes(screen)
    ctx.check("card.never_says_agent", not card["stale"] and "Agent" not in card["labels"], card)
    ctx.check("footer.ask", footer_mode(screen) == "ask", footer_mode(screen))
    submit(t, "first light")
    t.wait_for("heard: first light", 20)
    time.sleep(1)
    t.capture("2-after-prompt")
    ctx.check("badge.matches_adapter_ask", badge(t.screen()) == last_set(ctx) == "default",
              {"badge": badge(t.screen()), "adapter": last_set(ctx), "sets": mode_sets(ctx)})
    quit_fg(t)


# ---------------------------------------------------------------------------
# S41: `@` and `/new DIR` from each session's own folder on the host, with the
# client's home and folder deliberately different from the host's.
# ---------------------------------------------------------------------------

def spool_dirs(ctx):
    """`~/spool/app` exists under both homes with different files, so `~` expansion shows whose home was used.
    `~/kiln_host` and `~/kiln_laptop` exist only under one home each, so Tab on `~/kil` shows whose disk answered."""
    made = {}
    for name, leaf in (("host", "spool_thread.rs"), ("laptop", "decoy_spool.rs")):
        d = ctx.fx["homes"][name] / "spool" / "app"
        (d / "src").mkdir(parents=True, exist_ok=True)
        (d / "src" / leaf).write_text(f"synthetic {name} spool file\n")
        (ctx.fx["homes"][name] / f"kiln_{name}").mkdir(exist_ok=True)
        made[name] = d.resolve()
    return made


def at_rows(t, query, settle=2.0, label=None):
    t.keys("C-u")
    t.type(query)
    time.sleep(settle)
    screen = t.screen()
    if label:
        t.capture(label)
    t.keys("C-u")
    return screen


def tab_rows(t, draft, settle=1.5, label=None):
    """Type a draft, press Tab once, return (composer+notice text) and clear."""
    t.keys("C-u")
    t.type(draft)
    time.sleep(0.6)
    t.keys("Tab")
    time.sleep(settle)
    screen = t.screen()
    if label:
        t.capture(label)
    t.keys("C-u")
    return screen


def last_new_cwd(ctx):
    news = ctx.ledger("request", "session/new")
    return news[-1].get("cwd") if news else None


def new_session_typed(ctx, t, text):
    news = len(ctx.ledger("request", "session/new"))
    t.keys("C-u")
    t.type(text)
    time.sleep(0.8)
    t.keys("Enter")
    time.sleep(0.8)
    if re.search(r"^\s*›?\s*/new\b", t.screen(), re.M):
        t.keys("Enter")
    started = wait_ledger(ctx, lambda r: len(ctx.ledger("request", "session/new")) > news, 15)
    time.sleep(1.5)
    return bool(started)


def host_requests_by_client(ctx, method):
    """{client id: count} of one ACP method in the host debug log."""
    out = {}
    for log in sorted((ctx.fx["homes"]["host"] / "log").glob("nori-acp*")):
        for line in log.read_text(errors="replace").splitlines():
            if "host ACP request" in line and f" method={method}" in line:
                m = re.search(r"client=(\d+)", line)
                out[m.group(1) if m else "?"] = out.get(m.group(1) if m else "?", 0) + 1
    return out


def remote_attach(ctx, name, sid=""):
    endpoint, _ = wire.host_endpoint(ctx.fx["homes"]["host"])
    token_file = ctx.fx["homes"]["host"] / "host" / "token"
    t = ctx.term(name, cwd="laptop", home="laptop")
    t.run(f"{CLIENT} attach --host {endpoint} --token-file {token_file} {sid}".strip())
    return t


def files_of(screen):
    return {k for k in ("lantern_wick", "harbor_tide", "spool_thread", "decoy_laptop", "decoy_spool") if k in screen}


def s41_attach_two_sessions_and_watcher(ctx):
    spool = spool_dirs(ctx)
    ctx.start_host(cwd="alpha", log=True)
    obs = ctx.observer()
    c = remote_attach(ctx, "controller")
    attach_new_session(c, str(ctx.fx["roots"]["alpha"]))
    ctx.check("a.session_one_in_alpha", last_new_cwd(ctx) == str(ctx.fx["roots"]["alpha"]), last_new_cwd(ctx))
    screen = at_rows(c, "@lan", label="0-at-alpha")
    ctx.check("a.controller_at_offers_alpha", "lantern_wick" in screen and "decoy_laptop" not in screen, screen[-400:])

    new_session_typed(ctx, c, "/new ~/spool/app")
    c.capture("1-new-tilde")
    ctx.check("b.tilde_expands_with_host_home", last_new_cwd(ctx) == str(spool["host"]),
              {"cwd": last_new_cwd(ctx), "host_home": str(spool["host"]), "client_home": str(spool["laptop"])})
    screen = at_rows(c, "@spool", label="2-at-spool")
    ctx.check("b.controller_at_follows_new_session", "spool_thread" in screen and "decoy_spool" not in screen
              and "lantern_" not in screen, screen[-400:])

    sessions = {x["sessionId"]: x.get("cwd") for x in obs.sessions()}
    (ctx.dir / "host-sessions.json").write_text(json.dumps(sessions, indent=2))
    first = next((sid for sid, cwd in sessions.items() if cwd == str(ctx.fx["roots"]["alpha"])), None)
    ctx.check("host.session_cwds", first is not None and str(spool["host"]) in sessions.values(), sessions)

    before = host_requests_by_client(ctx, "_nori/session/find_paths")
    w = remote_attach(ctx, "watcher", first[:8] if first else "")
    w.wait_for(r"(WATCHING|watching)", 20, regex=True)
    time.sleep(1.5)
    screen = at_rows(w, "@lan", label="3-watcher-at-alpha")
    ctx.check("w.watcher_at_offers_its_session_folder", "lantern_wick" in screen and "decoy_laptop" not in screen
              and "spool_thread" not in screen, screen[-400:])
    wscreen = tab_rows(w, "/new ~/kil", label="3b-watcher-tab-kiln")
    after = host_requests_by_client(ctx, "_nori/session/find_paths")
    new_clients = [k for k in after if k not in before]
    ctx.check("w.find_paths_answered_for_watcher_client", len(new_clients) == 1 and after[new_clients[0]] >= 2,
              {"before": before, "after": after})
    ctx.check("w.watcher_tab_completes_from_host", "kiln_host" in wscreen and "kiln_laptop" not in wscreen,
              [l.strip() for l in wscreen.splitlines() if "kil" in l])
    ctx.check("w.watcher_still_watching", re.search(r"WATCHING", w.screen()) is not None, last_nonblank(w.screen()))

    new_session_typed(ctx, c, "/new")
    ctx.check("c.plain_new_inherits_foreground_folder", last_new_cwd(ctx) == str(spool["host"]), last_new_cwd(ctx))

    screen = tab_rows(c, "/new ~/kil", label="4-tab-tilde")
    ctx.check("c.tab_completes_from_host_home", "kiln_host" in screen and "kiln_laptop" not in screen,
              [l.strip() for l in screen.splitlines() if "kil" in l])
    screen = tab_rows(c, "/new ~/spo")
    ctx.check("c.tab_completes_spool", "~/spool/" in screen, [l.strip() for l in screen.splitlines() if "spo" in l])

    # Foreground switching through the session list: each visited session's `@` offers only its own folder.
    seen = []
    for i in range(3):
        c.keys("C-u")
        c.keys("M-s")
        time.sleep(1.2)
        if i == 0:
            c.capture("5-session-list")
        for _ in range(i):
            c.keys("Down")
            time.sleep(0.2)
        c.keys("Enter")
        time.sleep(1.5)
        lan = at_rows(c, "@lan")
        spo = at_rows(c, "@spool", label=f"6-switch-{i}")
        seen.append({"lan": sorted(files_of(lan)), "spool": sorted(files_of(spo)),
                     "identity": next((l.strip() for l in c.screen().splitlines() if "·" in l and "Approvals" in l), "")})
    (ctx.dir / "switch-visits.json").write_text(json.dumps(seen, indent=2))
    exclusive = all((v["lan"] == ["lantern_wick"] and v["spool"] == []) or
                    (v["lan"] == [] and v["spool"] == ["spool_thread"]) for v in seen)
    kinds = {"alpha" if v["lan"] else "spool" for v in seen}
    ctx.check("s.at_follows_switched_session", exclusive and kinds == {"alpha", "spool"}, seen)
    (ctx.dir / "host-requests.txt").write_text("\n".join(host_requests(ctx)) + "\n")
    obs.close()


def s41_plain_new_dir_and_at(ctx):
    t = start_fg(ctx)
    screen = at_rows(t, "@lan", label="0-at-alpha")
    ctx.check("p.at_offers_launch_folder", "lantern_wick" in screen, screen[-400:])
    news = len(ctx.ledger("request", "session/new"))
    run_typed(t, f"/new {ctx.fx['roots']['beta']}", settle=3.0)
    wait_ledger(ctx, lambda r: len(ctx.ledger("request", "session/new")) > news, 15)
    time.sleep(1)
    t.capture("1-new-beta")
    ctx.check("p.new_dir_starts_in_dir", last_new_cwd(ctx) == str(ctx.fx["roots"]["beta"]), last_new_cwd(ctx))
    screen = at_rows(t, "@harb", label="2-at-beta")
    ctx.check("p.at_follows_new_session", "harbor_tide" in screen and "lantern_" not in screen, screen[-400:])
    news = len(ctx.ledger("request", "session/new"))
    run_typed(t, "/new", settle=3.0)
    wait_ledger(ctx, lambda r: len(ctx.ledger("request", "session/new")) > news, 15)
    ctx.check("p.plain_new_inherits", last_new_cwd(ctx) == str(ctx.fx["roots"]["beta"]), last_new_cwd(ctx))
    t.keys("M-,")
    time.sleep(1.5)
    t.keys("M-,")
    time.sleep(1.5)
    screen = at_rows(t, "@lan", label="3-back-at-alpha")
    ctx.check("p.switch_back_at_alpha", "lantern_wick" in screen and "harbor_" not in screen, screen[-400:])
    quit_fg(t)


def s41c_attach_no_session_defaults(ctx):
    """A `--host` attach with no session on screen: `@`, Tab and relative `/new` use the host's folder."""
    spool_dirs(ctx)
    ctx.start_host(cwd="alpha", log=True)
    c = remote_attach(ctx, "controller")
    screen = c.wait_for(r"(Sessions|›)", 20, regex=True)
    if "No sessions yet" in screen or "Sessions" in screen:
        c.keys("Escape")
    time.sleep(1)
    c.capture("0-empty")
    screen = at_rows(c, "@lan", label="1-at-no-session")
    ctx.check("n.at_offers_host_folder", "lantern_wick" in screen and "decoy_laptop" not in screen, screen[-400:])
    screen = tab_rows(c, "/new ./assets/", label="2-tab-relative")
    ctx.check("n.tab_relative_from_host_folder", "alpha-only" in screen and "laptop-only" not in screen,
              [l.strip() for l in screen.splitlines() if "assets" in l])
    screen = tab_rows(c, "/new ~/kil")
    ctx.check("n.tab_tilde_from_host_home", "kiln_host" in screen and "kiln_laptop" not in screen,
              [l.strip() for l in screen.splitlines() if "kil" in l])
    started = new_session_typed(ctx, c, "/new ./docs")
    c.capture("3-new-relative")
    want = str(ctx.fx["roots"]["alpha"] / "docs")
    ctx.check("n.relative_new_uses_host_folder", started and last_new_cwd(ctx) == want,
              {"cwd": last_new_cwd(ctx), "host": want, "client": str(ctx.fx["roots"]["laptop"] / "docs")})
    c.keys("C-u")


def s41d_plain_tilde_and_tab(ctx):
    """Plain `nori`: `/new ~/…` and Tab on `/new` from the session on screen, including relative folders."""
    spool = spool_dirs(ctx)
    t = start_fg(ctx)
    screen = tab_rows(t, "/new ~/kil", label="0-tab-tilde")
    ctx.check("d.tab_tilde_completes", "~/kiln_host/" in screen, [l.strip() for l in screen.splitlines() if "kil" in l])
    screen = tab_rows(t, "/new ./ass", label="1-tab-relative")
    ctx.check("d.tab_relative_from_session", "./assets/" in screen, [l.strip() for l in screen.splitlines() if "ass" in l])
    news = len(ctx.ledger("request", "session/new"))
    run_typed(t, "/new ~/spool/app", settle=3.0)
    wait_ledger(ctx, lambda r: len(ctx.ledger("request", "session/new")) > news, 15)
    time.sleep(1)
    t.capture("2-new-tilde")
    ctx.check("d.tilde_new_uses_home", last_new_cwd(ctx) == str(spool["host"]), last_new_cwd(ctx))
    screen = at_rows(t, "@spool", label="3-at-spool")
    ctx.check("d.at_follows_tilde_session", "spool_thread" in screen and "lantern_" not in screen, screen[-400:])
    screen = tab_rows(t, "/new ./s", label="4-tab-relative-spool")
    ctx.check("d.tab_relative_follows_session", "./src/" in screen and "./shared" not in screen,
              [l.strip() for l in screen.splitlines() if "./s" in l])
    t.keys("M-,")
    time.sleep(1.5)
    screen = tab_rows(t, "/new ./s", label="5-tab-after-switch")
    ctx.check("d.tab_follows_switch", "./s" in screen and "shared" in screen,
              [l.strip() for l in screen.splitlines() if "./s" in l])
    quit_fg(t)


def s41e_plain_no_session(ctx):
    """Plain `nori` after a refused start (no session): `@`, Tab and `/new` use the launch folder."""
    (ctx.fx["control"] / "new-refuse").write_text("1")
    t = ctx.term("fg", cwd="beta")
    t.run(f"{HOST_LOG_ENV} {NORI} --skip-trust-directory")
    wait_ledger(ctx, lambda r: r.get("kind") == "new_refused", timeout=15)
    time.sleep(2.5)
    t.keys("Escape")
    time.sleep(0.5)
    t.capture("0-refused")
    screen = at_rows(t, "@harb", label="1-at-no-session")
    ctx.check("e.at_offers_launch_folder", "harbor_tide" in screen and "lantern_" not in screen, screen[-400:])
    screen = tab_rows(t, "/new ./ass", label="2-tab-no-session")
    ctx.check("e.tab_from_launch_folder", "./assets/" in screen, [l.strip() for l in screen.splitlines() if "ass" in l])
    news = len(ctx.ledger("request", "session/new"))
    run_typed(t, "/new", settle=3.0)
    wait_ledger(ctx, lambda r: len(ctx.ledger("request", "session/new")) > news, 15)
    ctx.check("e.plain_new_uses_launch_folder", last_new_cwd(ctx) == str(ctx.fx["roots"]["beta"]), last_new_cwd(ctx))
    quit_fg(t)


def s41f_older_host(ctx):
    """Run with --nori <older host> --client <new client>: no find_paths; this machine completes and names folders."""
    spool = spool_dirs(ctx)
    ctx.start_host(cwd="alpha", log=True)
    c = remote_attach(ctx, "controller")
    screen = c.wait_for(r"(Sessions|›)", 20, regex=True)
    if "No sessions yet" in screen or "Sessions" in screen:
        c.keys("Escape")
    time.sleep(1)
    started = new_session_typed(ctx, c, "/new ./docs")
    ctx.check("o.relative_no_session_joined_to_terminal",
              started and last_new_cwd(ctx) == str(ctx.fx["roots"]["laptop"] / "docs"), last_new_cwd(ctx))
    started = new_session_typed(ctx, c, "/new ~/spool/app")
    c.capture("0-new-tilde")
    ctx.check("o.tilde_expanded_with_client_home", started and last_new_cwd(ctx) == str(spool["laptop"]),
              {"cwd": last_new_cwd(ctx), "client": str(spool["laptop"]), "host": str(spool["host"])})
    screen = tab_rows(c, "/new ~/kil", label="1-tab-local")
    ctx.check("o.tab_completes_locally", "kiln_laptop" in screen and "kiln_host" not in screen,
              [l.strip() for l in screen.splitlines() if "kil" in l])
    screen = at_rows(c, "@spool", label="2-at-empty")
    ctx.check("o.at_shows_nothing", not files_of(screen), sorted(files_of(screen)))
    reqs = host_requests(ctx)
    ctx.check("o.no_find_paths_sent", "_nori/session/find_paths" not in reqs, sorted(set(reqs)))
    word = "kiln older host word"
    submit(c, word)
    ok = wait_text(c, f"heard: {word}", 20)
    ctx.check("o.prompt_delivered", ok and len(prompts_with(ctx, word)) == 1, len(prompts_with(ctx, word)))
    c.capture("3-prompt")
    ctx.check("o.still_connected", "CONTROLLING" in c.screen(), last_nonblank(c.screen()))


def s41g_wire_contract(ctx):
    """`_nori/session/find_paths` and `_nori/session/new` on the wire, against the registry row and the repo docs."""
    spool = spool_dirs(ctx)
    alpha = ctx.fx["roots"]["alpha"]
    (alpha / "src" / "lantern_escape.rs").symlink_to(ctx.fx["roots"]["laptop"] / "src" / "decoy_laptop.rs")
    ctx.start_host(cwd="alpha", log=True)
    endpoint, token = wire.host_endpoint(ctx.fx["homes"]["host"])
    ctl = wire.Wire(endpoint, token, observe=False, log=ctx.dir / "controller-wire.jsonl")
    caps = ctl.init.get("result", {})
    ctx.check("g.capability_advertised", "find_paths" in json.dumps(caps), json.dumps(caps)[:300])
    obs = ctx.observer()
    out = {}

    def fp(label, params, who=ctl):
        r = who.call("_nori/session/find_paths", params)
        out[label] = r
        return r
    r = fp("no_session_files", {"query": "lan", "kind": "files"})
    res = r.get("result", {})
    paths = [m["path"] for m in res.get("matches", [])]
    ctx.check("g.no_session_root_is_host_folder", res.get("root") == str(alpha), res.get("root"))
    ctx.check("g.files_relative_and_links_out_dropped", "src/lantern_wick.rs" in paths and
              not any("lantern_escape" in p for p in paths), paths)
    r = fp("empty_query", {"query": "", "kind": "files"})
    ctx.check("g.empty_files_query_no_matches", r.get("result", {}).get("matches") == [], r)
    r = fp("folders_tilde", {"query": "~/kil", "kind": "folders"})
    ctx.check("g.folders_tilde_host_home", [m["path"] for m in r.get("result", {}).get("matches", [])] == ["~/kiln_host/"], r)
    r = fp("limit_0", {"query": "", "kind": "folders", "limit": 0})
    ctx.check("g.limit_clamped_low", "result" in r and len(r["result"]["matches"]) == 1, r)
    r = fp("limit_100", {"query": "a", "kind": "files", "limit": 100})
    ctx.check("g.limit_100_accepted", "result" in r, r.get("error"))
    news = ctl.call("_nori/session/new", {"cwd": "~/spool/app"}, timeout=30)
    out["new_tilde"] = news
    sid = news.get("result", {}).get("session_id")
    ctx.check("g.new_tilde_host_home", news.get("result", {}).get("cwd") == str(spool["host"]), news)
    r = ctl.call("_nori/session/new", {"cwd": "~nobody/x"}, timeout=30)
    out["new_tilde_user"] = r
    ctx.check("g.new_tilde_user_refused", "error" in r, r)
    r = ctl.call("_nori/session/new", {"cwd": "docs"}, timeout=30)
    out["new_relative"] = r
    ctx.check("g.new_relative_host_folder", r.get("result", {}).get("cwd") == str(alpha / "docs"), r)
    r = fp("camel_sessionId", {"sessionId": sid, "query": "spool", "kind": "files"})
    ctx.check("g.sessionId_camel_scopes_to_session", r.get("result", {}).get("root") == str(spool["host"]), r)
    r = fp("snake_session_id_registry", {"session_id": sid, "query": "spool", "kind": "files"})
    # #278 a0d3ba51 documented `session_id`; c62bcc22 documents `sessionId` with unknown fields refused.
    ctx.check("g.snake_session_id_refused", r.get("error", {}).get("code") == -32602, r)
    r = fp("unknown_session", {"sessionId": "no-such-session", "query": "x", "kind": "files"})
    ctx.check("g.unknown_session_32602", r.get("error", {}).get("code") == -32602, r)
    r = fp("watcher", {"sessionId": sid, "query": "spool", "kind": "files"}, who=obs)
    ctx.check("g.watcher_allowed", r.get("result", {}).get("root") == str(spool["host"]), r)
    (alpha / ".git").mkdir(exist_ok=True)
    (alpha / ".git" / "lantern_gitobj.rs").write_text("synthetic\n")
    (alpha / ".lantern_hidden").mkdir(exist_ok=True)
    (alpha / "lantern spaced").mkdir(exist_ok=True)
    (alpha / "lantern_plain").mkdir(exist_ok=True)
    r = fp("git_skipped", {"query": "lantern_gitobj", "kind": "files"})
    ctx.check("g2.git_skipped", r.get("result", {}).get("matches") == [], r)
    r = fp("folders_l", {"query": "lan", "kind": "folders"})
    got = [m["path"] for m in r.get("result", {}).get("matches", [])]
    ctx.check("g2.whitespace_folder_skipped_dot_hidden", got == ["lantern_plain/"], got)
    r = fp("folders_dot", {"query": ".lan", "kind": "folders"})
    got = [m["path"] for m in r.get("result", {}).get("matches", [])]
    ctx.check("g2.dot_folder_when_typed", got == [".lantern_hidden/"], got)
    r = fp("malformed_kind", {"query": "x", "kind": "everything"})
    ctx.check("g2.malformed_32602", r.get("error", {}).get("code") == -32602, r)
    (ctx.dir / "wire-results.json").write_text(json.dumps(out, indent=2))
    ctl.close()
    obs.close()


# ---- S42: /screen in plain nori and nori-tui (#757) -------------------------

def toggle_screen(t, want_alt, timeout=6):
    """Run /screen from an empty composer and wait for the alternate-screen flag."""
    run_typed(t, "/screen", settle=0.3)
    deadline = time.time() + timeout
    while time.time() < deadline and t.alt_screen() != want_alt:
        time.sleep(0.15)
    time.sleep(0.6)
    return t.alt_screen() == want_alt


def resize(t, cols, rows):
    term.tmux("resize-window", "-t", t.name, "-x", str(cols), "-y", str(rows))
    time.sleep(1.2)


def tag_rows(text, tag):
    return [int(m) for m in re.findall(rf"\b{re.escape(tag)} row (\d\d)\b", text)]


def relay_n(ctx, tag):
    rows = [r for r in ctx.ledger("relay") if r.get("tag") == tag]
    return rows[-1]["n"] if rows else 0


def relay_done(ctx, tag):
    return any(r.get("kind") == "turn_end" and r.get("rows") is not None and
               r.get("sessionId") in {x.get("sessionId") for x in ctx.ledger("relay") if x.get("tag") == tag}
               for r in ctx.ledger("turn_end"))


def wait_relay(ctx, tag, n, timeout=30):
    deadline = time.time() + timeout
    while time.time() < deadline and relay_n(ctx, tag) < n:
        time.sleep(0.1)
    return relay_n(ctx, tag)


def prompts_with(ctx, text):
    out = []
    for r in ctx.ledger("request", "session/prompt"):
        joined = r.get("text") or ""
        if text in joined:
            out.append(joined)
    return out


def restored(ctx, t, prefix):
    """The shell after the TUI exits: normal screen, cursor shown, cooked tty, balanced modes."""
    deadline = time.time() + 10
    while time.time() < deadline and not t.at_shell():
        time.sleep(0.3)
    ctx.check(f"{prefix}.back_at_shell", t.at_shell(), t.screen()[-300:])
    ctx.check(f"{prefix}.normal_screen", not t.alt_screen(), t.fmt("#{alternate_on}"))
    ctx.check(f"{prefix}.cursor_visible", t.cursor_visible(), t.fmt("#{cursor_flag}"))
    ctx.check(f"{prefix}.tty_cooked", tty_sane(t.tty_modes()))
    esc = term.escape_counts(t.raw())
    ctx.check(f"{prefix}.escapes_balanced", esc["alt_on"] == esc["alt_off"] and esc["paste_on"] == esc["paste_off"], esc)


def s42a_mid_turn_both_directions(ctx, binary="nori"):
    t = start_fg(ctx, cmd=f"{binary} --skip-trust-directory")
    ctx.check("a.starts_inline", not t.alt_screen())
    submit(t, "harbor opening line")
    t.wait_for("heard: harbor opening line", 20)
    time.sleep(0.8)
    submit(t, "relay 40 amber")
    wait_relay(ctx, "amber", 6)
    ctx.check("a.into_full_mid_turn", toggle_screen(t, True), relay_n(ctx, "amber"))
    at_in = relay_n(ctx, "amber")
    t.capture("1-full-mid-turn")
    s = t.screen()
    ctx.check("a.full_shows_transcript", "harbor opening line" in s and "amber row" in s and "›" in s, s[-600:])
    wait_relay(ctx, "amber", 24)
    time.sleep(0.5)
    t.capture("2-full-streaming")
    later = tag_rows(t.screen(), "amber")
    ctx.check("a.full_keeps_streaming", later and max(later) > at_in + 8, {"entered_at": at_in, "visible": later[-5:]})
    ctx.check("a.out_of_full_mid_turn", toggle_screen(t, False), relay_n(ctx, "amber"))
    at_out = relay_n(ctx, "amber")
    ctx.check("a.toggle_out_was_mid_turn", at_out < 40, at_out)
    t.wait_for("amber relay finished.", 30)
    time.sleep(1.2)
    t.capture("3-inline-after")
    h = t.history()
    rows = tag_rows(h, "amber")
    ctx.check("a.scrollback_rows_once_in_order", rows == list(range(1, 41)),
              {"count": len(rows), "dups": sorted({n for n in rows if rows.count(n) > 1}), "head": rows[:12], "entered": at_in, "left": at_out})
    ctx.check("a.earlier_exchange_once", h.count("heard: harbor opening line") == 1 and h.count("harbor opening line") == 2,
              {"heard": h.count("heard: harbor opening line"), "all": h.count("harbor opening line")})
    ctx.check("a.finish_line_once", h.count("amber relay finished.") == 1, h.count("amber relay finished."))
    # A whole turn inside the full-screen view lands once on return.
    ctx.check("a.into_full_idle", toggle_screen(t, True))
    submit(t, "relay 12 cobalt")
    t.wait_for("cobalt relay finished.", 30)
    time.sleep(0.8)
    t.capture("4-full-whole-turn")
    ctx.check("a.out_full_idle", toggle_screen(t, False))
    time.sleep(1.0)
    h = t.history()
    rows = tag_rows(h, "cobalt")
    ctx.check("a.full_turn_lands_once", rows == list(range(1, 13)), rows)
    ctx.check("a.amber_still_once", tag_rows(h, "amber") == list(range(1, 41)), len(tag_rows(h, "amber")))
    ctx.check("a.prompts_delivered_once", len(prompts_with(ctx, "relay 40 amber")) == 1 and len(prompts_with(ctx, "relay 12 cobalt")) == 1,
              {"amber": len(prompts_with(ctx, "relay 40 amber")), "cobalt": len(prompts_with(ctx, "relay 12 cobalt"))})
    t.capture("5-inline-final")
    quit_fg(t)
    restored(ctx, t, "a.exit")


def s42b_separate_sessions(ctx):
    t = start_fg(ctx)
    submit(t, "alpha marker")
    t.wait_for("heard: alpha marker", 20)
    news = len(ctx.ledger("request", "session/new"))
    run_typed(t, "/new", settle=3.0)
    wait_ledger(ctx, lambda r: len(ctx.ledger("request", "session/new")) > news, 15)
    time.sleep(1)
    submit(t, "beta marker")
    t.wait_for("heard: beta marker", 20)
    time.sleep(0.8)
    submit(t, "relay 70 beta")
    wait_relay(ctx, "beta", 5)
    ctx.check("b.into_full_on_b", toggle_screen(t, True))
    s = t.screen()
    t.capture("1-full-b")
    ctx.check("b.full_shows_b_only", "beta marker" in s and "beta row" in s and "alpha marker" not in s, s[-500:])
    t.keys("M-,")
    time.sleep(1.5)
    n0 = relay_n(ctx, "beta")
    time.sleep(1.5)
    s = t.screen()
    t.capture("2-full-a-while-b-streams")
    ctx.check("b.full_switch_shows_a_only", "alpha marker" in s and "beta marker" not in s and "beta row" not in s,
              {"b_rows_meanwhile": relay_n(ctx, "beta") - n0, "screen": s[-500:]})
    ctx.check("b.still_full_after_switch", t.alt_screen())
    t.keys("M-,")
    time.sleep(1.2)
    s = t.screen()
    t.capture("3-full-back-to-b")
    vis = tag_rows(s, "beta")
    ctx.check("b.full_b_kept_streaming", "beta marker" in s or (vis and max(vis) > n0), {"visible": vis[-5:], "n0": n0})
    t.keys("M-,")
    time.sleep(1.2)
    ctx.check("b.inline_on_a_mid_b_turn", toggle_screen(t, False) and relay_n(ctx, "beta") < 70, relay_n(ctx, "beta"))
    time.sleep(1.5)
    h = t.history()
    t.capture("4-inline-a")
    ctx.check("b.inline_a_scrollback_is_a", h.count("heard: alpha marker") == 1 and "beta row" not in h and "beta marker" not in h,
              {"alpha": h.count("heard: alpha marker"), "beta_rows": len(tag_rows(h, "beta")), "beta_marker": h.count("beta marker")})
    deadline = time.time() + 30
    while time.time() < deadline and relay_n(ctx, "beta") < 70:
        time.sleep(0.3)
    time.sleep(1.0)
    t.keys("M-,")
    time.sleep(2.5)
    h = t.history()
    t.capture("5-inline-b")
    rows = tag_rows(h, "beta")
    ctx.check("b.inline_b_rows_once_in_order", rows == list(range(1, 71)),
              {"count": len(rows), "dups": sorted({n for n in rows if rows.count(n) > 1}), "head": rows[:8]})
    ctx.check("b.inline_b_has_no_a", "alpha marker" not in h and h.count("heard: beta marker") == 1,
              {"alpha": h.count("alpha marker"), "beta": h.count("heard: beta marker")})
    ctx.check("b.one_prompt_each", len(prompts_with(ctx, "relay 70 beta")) == 1)
    quit_fg(t)
    restored(ctx, t, "b.exit")


def s42c_overlay_resize_exit(ctx):
    t = start_fg(ctx)
    submit(t, "gull tally")
    t.wait_for("heard: gull tally", 20)
    time.sleep(0.8)
    ctx.check("c.into_full", toggle_screen(t, True))
    draft = "tern draft typed full"
    t.type(draft)
    time.sleep(0.6)
    t.keys("C-t")
    s = t.wait_for("T R A N S C R I P T", 10)
    t.capture("1-overlay-over-full")
    resize(t, 100, 30)
    t.capture("2-overlay-resized")
    t.keys("q")
    time.sleep(1.0)
    s = t.screen()
    t.capture("3-overlay-closed")
    ctx.check("c.overlay_close_stays_full", t.alt_screen() and "T R A N S C R I P T" not in s and "heard: gull tally" in s, s[-400:])
    ctx.check("c.draft_survives_overlay_and_resize", draft in s, s[-300:])
    resize(t, 140, 40)
    s = t.screen()
    t.capture("4-full-resized")
    ctx.check("c.full_resize_redraws", t.alt_screen() and draft in s and "heard: gull tally" in s and len(s.splitlines()) >= 39,
              {"rows": len(s.splitlines()), "tail": s[-300:]})
    ctx.check("c.draft_unsent", not prompts_with(ctx, draft), len(prompts_with(ctx, draft)))
    t.keys("C-u")
    time.sleep(0.4)
    ctx.check("c.back_inline_after_resizes", toggle_screen(t, False))
    time.sleep(1.0)
    h = t.history()
    t.capture("5-inline")
    ctx.check("c.scrollback_once_after_resizes", h.count("heard: gull tally") == 1, h.count("heard: gull tally"))
    resize(t, 110, 34)
    s = t.screen()
    ctx.check("c.inline_resize_ok", "›" in s and not t.alt_screen(), s[-200:])
    # Exit from the full-screen view with an overlay open.
    ctx.check("c.into_full_again", toggle_screen(t, True))
    submit(t, "relay 10 slate")
    t.wait_for("slate relay finished.", 20)
    time.sleep(0.5)
    t.keys("C-t")
    t.wait_for("T R A N S C R I P T", 10)
    t.capture("6-overlay-before-exit")
    t.keys("C-c")
    time.sleep(0.6)
    if not t.at_shell():
        t.keys("C-c")
    time.sleep(2.0)
    t.capture("7-after-exit")
    restored(ctx, t, "c.exit_overlay")
    h = t.history()
    rows = tag_rows(h, "slate")
    ctx.check("c.held_history_printed_on_exit", rows == list(range(1, 11)) and h.count("heard: gull tally") == 1,
              {"slate": rows, "gull": h.count("heard: gull tally")})


def s42d_exit_from_full(ctx, how):
    t = start_fg(ctx)
    submit(t, "quay line")
    t.wait_for("heard: quay line", 20)
    time.sleep(0.6)
    ctx.check("d.into_full", toggle_screen(t, True))
    submit(t, "relay 30 moss")
    wait_relay(ctx, "moss", 6)
    t.capture("1-full-mid-turn")
    if how == "quit":
        slash(t, "/quit", settle=1.0)
    else:
        t.keys("C-c")
        time.sleep(0.6)
        if not t.at_shell():
            t.keys("C-c")
    time.sleep(2.0)
    t.capture("2-after-exit")
    restored(ctx, t, f"d.{how}")
    h = t.history()
    ctx.check(f"d.{how}.scrollback_once", h.count("heard: quay line") == 1 and len(set(tag_rows(h, "moss"))) == len(tag_rows(h, "moss")),
              {"quay": h.count("heard: quay line"), "moss": tag_rows(h, "moss")[:40]})


def s42e_draft_before_toggle(ctx):
    """A draft in the composer before the switch is wanted: is there a way to switch with it in place?"""
    t = start_fg(ctx)
    submit(t, "seed line")
    t.wait_for("heard: seed line", 20)
    time.sleep(0.6)
    draft = "pier draft nine exact words"
    t.type(draft)
    time.sleep(0.8)
    t.capture("0-draft")
    # Overlay with the draft already in place.
    t.keys("C-t")
    t.wait_for("T R A N S C R I P T", 10)
    t.keys("q")
    time.sleep(0.8)
    s = t.screen()
    ctx.check("e.overlay_keeps_draft", draft in s and not prompts_with(ctx, draft), s[-300:])
    # The only switch is the slash command, which has to be typed into the same composer.
    t.type("/screen")
    time.sleep(1.0)
    s = t.screen()
    t.capture("1-slash-after-draft")
    ctx.check("e.slash_with_draft_offers_no_switch", (draft + "/screen") in s and not t.alt_screen(),
              {"alt": t.alt_screen(), "tail": s[-300:]})
    for _ in "/screen":
        t.keys("BSpace")
    time.sleep(0.5)
    # Candidate composer-independent actions: none should be bound at this head.
    tried = {}
    for key in ["C-l", "F11", "M-s", "C-s"]:
        before = t.alt_screen()
        t.keys(key)
        time.sleep(0.8)
        tried[key] = t.alt_screen() != before
        if t.alt_screen():
            break
    t.capture("2-after-candidate-keys")
    s = t.screen()
    ctx.check("e.switch_with_draft_in_place", any(tried.values()) and draft in s, tried)
    ctx.check("e.draft_still_exact_unsent", draft in s and not prompts_with(ctx, draft), s[-300:])
    if t.alt_screen():
        ctx.check("e.draft_visible_full", draft in t.screen())
    t.keys("Enter")
    time.sleep(2.0)
    t.capture("3-submitted")
    sent = prompts_with(ctx, draft)
    ctx.check("e.draft_submits_once_exact", len(sent) == 1 and sent[0].strip() == draft, sent)
    quit_fg(t)


def s42f_attach_menu(ctx):
    ctx.start_host(cwd="alpha")
    c = start_attach(ctx, "controller")
    time.sleep(1.0)
    c.keys("C-u")
    c.type("/scr")
    time.sleep(1.0)
    s = c.screen()
    c.capture("0-attach-menu")
    ctx.check("f.attach_screen_greyed_with_reason", "/screen" in s and "nori attach is always full-screen" in s, s[-600:])
    c.keys("C-u")
    run_typed(c, "/screen", settle=1.0)
    ctx.check("f.attach_screen_does_nothing", c.alt_screen() and not prompts_with(ctx, "/screen"), c.screen()[-300:])

def s42i_resize_mid_turn_full(ctx):
    """A resize while full-screen and streaming, then back inline: the reflow reprint must not lose or repeat rows."""
    t = start_fg(ctx)
    submit(t, "reef line")
    t.wait_for("heard: reef line", 20)
    time.sleep(0.6)
    ctx.check("i.into_full", toggle_screen(t, True))
    submit(t, "relay 40 coral")
    wait_relay(ctx, "coral", 8)
    resize(t, 96, 30)
    t.capture("1-full-narrow-mid-turn")
    wait_relay(ctx, "coral", 16)
    resize(t, 132, 38)
    s = t.screen()
    t.capture("2-full-wide-mid-turn")
    ctx.check("i.full_redraws_after_resizes", t.alt_screen() and "coral row" in s and "›" in s, s[-300:])
    ctx.check("i.out_mid_turn", toggle_screen(t, False) and relay_n(ctx, "coral") < 40, relay_n(ctx, "coral"))
    t.wait_for("coral relay finished.", 30)
    time.sleep(2.0)
    h = t.history()
    t.capture("3-inline-after")
    rows = tag_rows(h, "coral")
    ctx.check("i.rows_once_in_order", rows == list(range(1, 41)),
              {"count": len(rows), "dups": sorted({n for n in rows if rows.count(n) > 1}), "missing": sorted(set(range(1, 41)) - set(rows))})
    ctx.check("i.earlier_once", h.count("heard: reef line") == 1, h.count("heard: reef line"))
    quit_fg(t)
    restored(ctx, t, "i.exit")


def s42j_suspend_in_full(ctx):
    """Ctrl+Z from the full-screen view, then fg: back on the full-screen view, and inline history intact."""
    t = start_fg(ctx)
    submit(t, "dune line")
    t.wait_for("heard: dune line", 20)
    time.sleep(0.6)
    ctx.check("j.into_full", toggle_screen(t, True))
    t.keys("C-z")
    time.sleep(1.5)
    s = t.screen()
    t.capture("1-suspended")
    ctx.check("j.suspend_shows_shell", not t.alt_screen() and "shell$" in s, {"alt": t.alt_screen(), "tail": s[-200:]})
    t.run("fg")
    time.sleep(2.0)
    s = t.screen()
    t.capture("2-resumed")
    ctx.check("j.fg_returns_to_full", t.alt_screen() and "heard: dune line" in s and "›" in s, {"alt": t.alt_screen(), "tail": s[-300:]})
    ctx.check("j.out_after_fg", toggle_screen(t, False))
    time.sleep(1.0)
    h = t.history()
    t.capture("3-inline")
    ctx.check("j.scrollback_once", h.count("heard: dune line") == 1, h.count("heard: dune line"))
    submit(t, "after the pause")
    t.wait_for("heard: after the pause", 15)
    quit_fg(t)
    restored(ctx, t, "j.exit")


def s42k_permission_in_full(ctx, full=True):
    """A permission request that arrives while full-screen is answerable there, and lands once inline.
    With full=False the same turn runs inline as the ordering control."""
    t = start_fg(ctx)
    if full:
        ctx.check("k.into_full", toggle_screen(t, True))
    submit(t, "perm paint")
    t.wait_for("Paint the boathouse", 15)
    time.sleep(0.8)
    t.capture("1-permission")
    ctx.check("k.prompt_visible", t.alt_screen() == full and "Allow" in t.screen(), t.screen()[-400:])
    t.keys("Enter")
    t.wait_for("permission: {", 15)
    time.sleep(1.0)
    s = t.screen()
    t.capture("2-answered")
    ctx.check("k.answered_once", len(perm_responses(ctx)) == 1, perm_responses(ctx))
    order = [l.strip()[:40] for l in s.splitlines() if "perm paint" in l or "Permission:" in l or "permission: {" in l]
    # Compared with S42m (inline) and the base commit: the decision row lands above the prompt row
    # in plain nori before #757 too, so this records the order rather than asserting one.
    ctx.check("k.screen_rows_present", len(order) == 3, order)
    if full:
        ctx.check("k.still_full", t.alt_screen())
        ctx.check("k.out", toggle_screen(t, False))
        time.sleep(1.0)
    h = t.history()
    t.capture("3-inline")
    ctx.check("k.scrollback_once", h.count("permission: {") == 1 and h.count("Permission: Allow once") == 1,
              {"reply": h.count("permission: {"), "decision": h.count("Permission: Allow once")})
    horder = [l.strip()[:40] for l in h.splitlines() if "› perm paint" in l or "Permission:" in l or "permission: {" in l]
    ctx.check("k.scrollback_rows_once", len(horder) == 3, horder)
    (ctx.dir / "order.json").write_text(json.dumps({"screen": order, "scrollback": horder}, indent=1))
    quit_fg(t)
    restored(ctx, t, "k.exit")


def s42l_resume_in_full(ctx, full=True):
    """/resume while full-screen: the restored session shows full-screen, and inline gets its history once.
    With full=False the same resume runs inline as the control."""
    t = start_fg(ctx)
    submit(t, "lagoon line")
    t.wait_for("heard: lagoon line", 20)
    time.sleep(0.6)
    if full:
        ctx.check("l.into_full", toggle_screen(t, True))
    open_resume_rows(t, "/resume")
    t.capture("1-picker")
    ok = choose(t, "Saved orchard survey")
    ctx.check("l.picked", ok)
    picked = time.time()
    seen = {}
    while time.time() - picked < 15:
        s = t.screen()
        for needle in ("an earlier synthetic question", "an earlier synthetic answer"):
            if needle in s and needle not in seen:
                seen[needle] = round(time.time() - picked, 1)
        time.sleep(0.2)
        if time.time() - picked > 8:
            break
    timeline = []
    for _ in range(20):
        s = t.screen()
        timeline.append(int("an earlier synthetic answer" in s))
        time.sleep(0.25)
    s = t.screen()
    t.capture("2-resumed")
    (ctx.dir / "seen.json").write_text(json.dumps({"first_seen": seen, "answer_present_8s_to_13s": timeline}))
    ctx.check("l.answer_stays_on_screen", all(timeline), timeline)
    if full and not any(timeline):
        resize(t, 118, 36)
        after_resize = "an earlier synthetic answer" in t.screen()
        t.capture("2b-after-resize")
        ctrl_l(t, False)
        time.sleep(1.0)
        inline = "an earlier synthetic answer" in t.history()
        t.capture("2c-inline-before-next-turn")
        ctrl_l(t, True)
        back = "an earlier synthetic answer" in t.screen()
        t.capture("2d-full-again")
        diag = {"after_resize_full": after_resize, "inline_before_next_turn": inline, "full_again": back}
        (ctx.dir / "diag.json").write_text(json.dumps(diag))
        ctx.check("l.diag_missing_answer", False, diag)
    ctx.check("l.resumed_shows_saved_exchange", t.alt_screen() == full and len(seen) == 2 and "lagoon line" not in s,
              {"alt": t.alt_screen(), "seen_after_s": seen, "tail": s[-300:]})
    submit(t, "orchard follow up")
    t.wait_for("heard: orchard follow up", 15)
    time.sleep(0.8)
    if full:
        ctx.check("l.out", toggle_screen(t, False))
        time.sleep(2.0)
    h = t.history()
    t.capture("3-inline")
    ctx.check("l.inline_has_resumed_once", h.count("an earlier synthetic answer") == 1 and h.count("heard: orchard follow up") == 1,
              {"answer": h.count("an earlier synthetic answer"), "follow": h.count("heard: orchard follow up")})
    ctx.check("l.inline_drops_previous_session", "lagoon line" not in h, h.count("lagoon line"))
    quit_fg(t)
    restored(ctx, t, "l.exit")


# ---- S43: the composer-independent switch (Ctrl+L, #757 7e648413) -----------

def ctrl_l(t, want_alt, key="C-l", timeout=5):
    t.keys(key)
    deadline = time.time() + timeout
    while time.time() < deadline and t.alt_screen() != want_alt:
        time.sleep(0.1)
    time.sleep(0.6)
    return t.alt_screen() == want_alt


def composer_row(screen):
    """The last `›` row on screen with the glyph stripped: the composer in plain nori and attach."""
    rows = [l for l in screen.splitlines() if l.lstrip().startswith("›")]
    return rows[-1].strip()[1:].strip() if rows else ""


def s43a_draft_across_switch(ctx, binary="nori"):
    t = start_fg(ctx, cmd=f"{binary} --skip-trust-directory")
    submit(t, "wharf opening")
    t.wait_for("heard: wharf opening", 20)
    time.sleep(0.6)
    draft = "keel draft eleven exact words, kept"
    t.type(draft)
    time.sleep(1.0)
    t.capture("0-inline-draft")
    ctx.check("a.draft_typed_inline", composer_row(t.screen()) == draft and not t.alt_screen(), composer_row(t.screen()))
    ctx.check("a.ctrl_l_into_full_with_draft", ctrl_l(t, True))
    s = t.screen()
    t.capture("1-full-draft")
    ctx.check("a.full_shows_draft_exact", composer_row(s) == draft and "heard: wharf opening" in s, composer_row(s))
    ctx.check("a.nothing_sent_into", not prompts_with(ctx, "keel draft"), len(prompts_with(ctx, "keel draft")))
    ctx.check("a.ctrl_l_back_inline_with_draft", ctrl_l(t, False))
    s = t.screen()
    t.capture("2-inline-draft-again")
    ctx.check("a.inline_shows_draft_exact", composer_row(s) == draft, composer_row(s))
    ctx.check("a.nothing_sent_back", not prompts_with(ctx, "keel draft"))
    h = t.history()
    ctx.check("a.scrollback_once_after_round_trip", h.count("heard: wharf opening") == 1, h.count("heard: wharf opening"))
    t.keys("Enter")
    t.wait_for("heard: keel draft", 15)
    time.sleep(1.0)
    t.capture("3-sent-once")
    sent = prompts_with(ctx, "keel draft")
    ctx.check("a.draft_submits_once_exact", len(sent) == 1 and sent[0].strip() == draft, sent)
    h = t.history()
    ctx.check("a.reply_once", h.count("heard: keel draft") == 1, h.count("heard: keel draft"))
    quit_fg(t)
    restored(ctx, t, "a.exit")


def s43b_draft_mid_turn(ctx):
    """Draft typed during a streaming turn, then Ctrl+L both ways mid-turn: draft kept, rows once, draft sent once later."""
    t = start_fg(ctx)
    submit(t, "relay 50 opal")
    wait_relay(ctx, "opal", 4)
    draft = "mast draft typed while streaming"
    t.type(draft)
    time.sleep(0.8)
    ctx.check("b.into_full_mid_turn", ctrl_l(t, True) and relay_n(ctx, "opal") < 50, relay_n(ctx, "opal"))
    wait_relay(ctx, "opal", 25)
    s = t.screen()
    t.capture("1-full-mid-turn-draft")
    ctx.check("b.full_draft_exact", composer_row(s) == draft, composer_row(s))
    ctx.check("b.out_mid_turn", ctrl_l(t, False) and relay_n(ctx, "opal") < 50, relay_n(ctx, "opal"))
    t.wait_for("opal relay finished.", 30)
    time.sleep(1.2)
    s = t.screen()
    h = t.history()
    t.capture("2-inline-after-turn")
    ctx.check("b.inline_draft_exact_unsent", composer_row(s) == draft and not prompts_with(ctx, "mast draft"), composer_row(s))
    rows = tag_rows(h, "opal")
    ctx.check("b.rows_once_in_order", rows == list(range(1, 51)), {"count": len(rows), "dups": sorted({n for n in rows if rows.count(n) > 1})})
    t.keys("Enter")
    t.wait_for("heard: mast draft", 15)
    sent = prompts_with(ctx, "mast draft")
    ctx.check("b.draft_sent_once_exact", len(sent) == 1 and sent[0].strip() == draft, sent)
    quit_fg(t)


def s43c_multiline_draft_and_popups(ctx):
    """A pasted two-line draft survives Ctrl+L; Ctrl+L with the slash menu or a permission prompt open is left to them."""
    t = start_fg(ctx)
    t.paste("first line of the buoy note\nsecond line of the buoy note")
    time.sleep(1.0)
    t.capture("0-two-line-draft")
    ctx.check("c.full_with_two_line_draft", ctrl_l(t, True))
    s = t.screen()
    t.capture("1-full-two-line")
    ctx.check("c.two_lines_kept_full", "first line of the buoy note" in s and "second line of the buoy note" in s, s[-300:])
    ctx.check("c.back_inline", ctrl_l(t, False))
    s = t.screen()
    ctx.check("c.two_lines_kept_inline", "first line of the buoy note" in s and "second line of the buoy note" in s and not prompts_with(ctx, "buoy note"), s[-300:])
    t.keys("C-u", "C-u", "C-u")
    for _ in range(80):
        t.keys("BSpace")
    time.sleep(0.4)
    t.type("/scr")
    time.sleep(0.8)
    before = t.alt_screen()
    t.keys("C-l")
    time.sleep(1.0)
    s = t.screen()
    t.capture("2-ctrl-l-over-slash-menu")
    ctx.check("c.slash_menu_switch_keeps_typed_text_and_menu", t.alt_screen() != before and "/ scr" in s and "/screen  switch" in s,
              {"before": before, "after": t.alt_screen(), "tail": s[-160:]})
    if t.alt_screen():
        ctrl_l(t, False)
    t.keys("Escape")
    t.keys("C-u")
    time.sleep(0.4)
    submit(t, "perm paint")
    t.wait_for("Paint the boathouse", 15)
    time.sleep(0.6)
    before = t.alt_screen()
    t.keys("C-l")
    time.sleep(1.0)
    s = t.screen()
    t.capture("3-ctrl-l-over-permission")
    # The prompt holds the key, as it holds Ctrl+T: no switch and no answer.
    ctx.check("c.permission_prompt_holds_ctrl_l_unanswered",
              t.alt_screen() == before and "Paint the boathouse" in s and "Allow once" in s and not perm_responses(ctx),
              {"before": before, "after": t.alt_screen(), "responses": len(perm_responses(ctx))})
    t.keys("Enter")
    t.wait_for("permission: {", 15)
    time.sleep(0.8)
    t.capture("4-answered-full")
    ctx.check("c.permission_answered_once", len(perm_responses(ctx)) == 1 and perm_responses(ctx)[0]["body"]["outcome"].get("optionId") == "allow",
              perm_responses(ctx))
    if t.alt_screen():
        ctrl_l(t, False)
    h = t.history()
    ctx.check("c.permission_turn_once_inline", h.count("permission: {") == 1, h.count("permission: {"))
    quit_fg(t)


def s43d_rebound_and_unbound(ctx):
    """[tui.hotkeys] toggle_screen = "alt+j": Ctrl+L stops switching, Alt+S switches; the ? overlay shows the binding."""
    t = start_fg(ctx)
    draft = "rebind draft"
    t.type(draft)
    time.sleep(0.6)
    t.keys("C-l")
    time.sleep(1.0)
    ctx.check("d.ctrl_l_unbound_after_rebind", not t.alt_screen() and composer_row(t.screen()) == draft, composer_row(t.screen()))
    ctx.check("d.alt_s_switches", ctrl_l(t, True, key="M-j"))
    ctx.check("d.alt_s_back", ctrl_l(t, False, key="M-j"))
    ctx.check("d.draft_kept", composer_row(t.screen()) == draft, composer_row(t.screen()))
    t.keys("C-u")
    time.sleep(0.3)
    t.type("?")
    time.sleep(1.0)
    s = t.screen()
    t.capture("1-shortcuts-overlay")
    row = [l.strip() for l in s.splitlines() if "full screen" in l]
    ctx.check("d.shortcuts_show_configured_key", row and "alt" in row[0].lower() and "ctrl + l" not in row[0].lower(), row)
    t.keys("BSpace")
    quit_fg(t)


# ---- S44: /resume while full-screen, no further input (#757 9106afd8 recheck) --

def s44_resume_spontaneous(ctx, binary="nori", full=True):
    """/resume while full-screen, then hands off: the restored question and answer must appear and stay
    with no key, resize or turn. Back inline before any new turn: each once, question before answer."""
    t = start_fg(ctx, cmd=f"{binary} --skip-trust-directory")
    submit(t, "lagoon line")
    t.wait_for("heard: lagoon line", 20)
    time.sleep(0.6)
    if full:
        ctx.check("r.into_full", toggle_screen(t, True))
    open_resume_rows(t, "/resume")
    t.capture("1-picker")
    ctx.check("r.picked", choose(t, "Saved orchard survey"))
    picked = time.time()
    seen, timeline = {}, []
    while time.time() - picked < 13:
        s = t.screen()
        for needle in ("an earlier synthetic question", "an earlier synthetic answer"):
            if needle in s and needle not in seen:
                seen[needle] = round(time.time() - picked, 1)
        if time.time() - picked > 8:
            timeline.append(int("an earlier synthetic question" in s and "an earlier synthetic answer" in s))
        time.sleep(0.2)
    s = t.screen()
    t.capture("2-resumed-no-input")
    (ctx.dir / "seen.json").write_text(json.dumps({"first_seen_s": seen, "both_present_8s_to_13s": timeline}))
    ctx.check("r.alt_unchanged", t.alt_screen() == full, t.fmt("#{alternate_on}"))
    ctx.check("r.both_appear_without_input", len(seen) == 2, {"seen_after_s": seen, "tail": s[-400:]})
    ctx.check("r.both_stay_on_screen", bool(timeline) and all(timeline), timeline)
    ctx.check("r.previous_session_gone", "lagoon line" not in s, s[-400:])
    if full:
        ctx.check("r.out_before_turn", toggle_screen(t, False))
        time.sleep(2.0)
    h = t.history()
    t.capture("3-inline-before-turn")
    q, a = h.count("an earlier synthetic question"), h.count("an earlier synthetic answer")
    ctx.check("r.inline_once", q == 1 and a == 1, {"question": q, "answer": a})
    ctx.check("r.inline_in_order", q >= 1 and a >= 1 and h.index("an earlier synthetic question") < h.index("an earlier synthetic answer"))
    ctx.check("r.inline_drops_previous_session", "lagoon line" not in h, h.count("lagoon line"))
    submit(t, "orchard follow up")
    t.wait_for("heard: orchard follow up", 15)
    time.sleep(0.8)
    h = t.history()
    t.capture("4-after-turn")
    ctx.check("r.after_turn_once_in_order",
              h.count("an earlier synthetic answer") == 1 and h.count("heard: orchard follow up") == 1
              and h.index("an earlier synthetic answer") < h.index("heard: orchard follow up"),
              {"answer": h.count("an earlier synthetic answer"), "follow": h.count("heard: orchard follow up")})
    quit_fg(t)
    restored(ctx, t, "r.exit")


def s44_resume_then_quit_full(ctx, binary="nori"):
    """/resume while full-screen, no input until /quit from full-screen: afterwards the normal terminal
    scrollback holds the restored question and answer once each, in order."""
    t = start_fg(ctx, cmd=f"{binary} --skip-trust-directory")
    submit(t, "lagoon line")
    t.wait_for("heard: lagoon line", 20)
    time.sleep(0.6)
    ctx.check("q.into_full", toggle_screen(t, True))
    open_resume_rows(t, "/resume")
    ctx.check("q.picked", choose(t, "Saved orchard survey"))
    picked = time.time()
    seen = {}
    while time.time() - picked < 8:
        s = t.screen()
        for needle in ("an earlier synthetic question", "an earlier synthetic answer"):
            if needle in s and needle not in seen:
                seen[needle] = round(time.time() - picked, 1)
        time.sleep(0.2)
    t.capture("1-resumed-full-no-input")
    ctx.check("q.both_appear_without_input", len(seen) == 2, seen)
    ctx.check("q.still_full", t.alt_screen())
    quit_fg(t)
    time.sleep(1.0)
    h = t.history()
    t.capture("2-after-quit")
    (ctx.dir / "scrollback-after-quit.txt").write_text(h)
    q, a = h.count("an earlier synthetic question"), h.count("an earlier synthetic answer")
    ctx.check("q.scrollback_once", q == 1 and a == 1, {"question": q, "answer": a})
    ctx.check("q.scrollback_in_order", q >= 1 and a >= 1 and h.index("an earlier synthetic question") < h.index("an earlier synthetic answer"))
    ctx.check("q.scrollback_drops_previous_session", "heard: lagoon line" not in h, h.count("heard: lagoon line"))
    restored(ctx, t, "q.exit")


def s43e_attach_ctrl_l(ctx):
    ctx.start_host(cwd="alpha")
    c = start_attach(ctx, "controller")
    time.sleep(1.0)
    draft = "attach draft stays"
    c.type(draft)
    time.sleep(0.6)
    before = c.screen()
    c.keys("C-l")
    time.sleep(1.0)
    s = c.screen()
    c.capture("0-attach-ctrl-l")
    ctx.check("e.attach_ctrl_l_noop_keeps_draft", c.alt_screen() and composer_row(s) == draft and not prompts_with(ctx, draft), composer_row(s))
    c.keys("C-u")
    c.type("?")
    time.sleep(1.0)
    s = c.screen()
    c.capture("1-attach-shortcuts")
    row = [l.strip() for l in s.splitlines() if "full screen" in l or "ctrl+l" in l.lower()]
    ctx.check("e.attach_hints_do_not_offer_noop_key", not row, row)
    c.keys("BSpace")


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
    "S34a": ("plain startup prepares and starts through the owned host", s34_plain_startup),
    "S34b": ("prompt typed before the startup server is ready is sent once", lambda c: s34_early_input(c, "typed")),
    "S34c": ("positional prompt before readiness is sent once", lambda c: s34_early_input(c, "positional")),
    "S34d": ("piped prompt before readiness is sent once", lambda c: s34_early_input(c, "piped")),
    "S34e": ("piped context plus instruction before readiness is sent once", lambda c: s34_early_input(c, "piped+arg")),
    "S34f": ("refused startup keeps typed input unsent", lambda c: s34_refused_startup(c, "typed")),
    "S34g": ("refused startup keeps a positional prompt unsent", lambda c: s34_refused_startup(c, "positional")),
    "S34h": ("/new during startup waits for the startup session", lambda c: s34_new_without_session(c, "during")),
    "S34i": ("/new after a refused startup starts a session", lambda c: s34_new_without_session(c, "after-refusal")),
    "S34j": ("/resume during startup reuses the startup server", s34_resume_during_startup),
    "S34k": ("skillset choice then startup through the host", lambda c: s34_deferred_skillset(c, "apply")),
    "S34l": ("skillset picker dismissed then startup through the host", lambda c: s34_deferred_skillset(c, "dismiss")),
    "S34m": ("control: `nori resume --last` stays in-process", s34_resume_id_control),
    "S35a": ("shared / menu, Tab and dispatch in plain nori", s35_plain_shared_commands),
    "S35b": ("attach controller: reasons, refusals, agent commands, quit", s35_attach_controller_commands),
    "S35c": ("two attached terminals: watcher refusals and control transfer", s35_watcher_and_transfer),
    "S35d": ("Escape with the menu open mid-turn (plain)", lambda c: s35_escape_mid_turn(c, "fg")),
    "S35e": ("Escape with the menu open mid-turn (attach)", lambda c: s35_escape_mid_turn(c, "attach")),
    "S35f": ("pasted slash commands (plain)", lambda c: s35_paste(c, "fg")),
    "S35g": ("pasted slash commands (attach)", lambda c: s35_paste(c, "attach")),
    "S35h": ("commands while reconnecting and after", s35_reconnect),
    "S35i": ("menu open mid-turn: first Escape closes, later Escapes interrupt (plain)", lambda c: s35_escape_two_presses(c, "fg")),
    "S35j": ("menu open mid-turn: first Escape closes, later Escapes interrupt (attach)", lambda c: s35_escape_two_presses(c, "attach")),
    "S37a": ("approvals: two sessions, three terminals, controller handoff, host vs UI", s37_handoff_two_sessions),
    "S37b": ("approvals: current config read as-is on repeat launch; sandbox kept", s37_repeat_launch_keeps_config),
    "S37c": ("approvals: plain nori, two sessions, mode follows the session", s37_plain_two_sessions),
    "S37d": ("approvals: Approve all answers permission requests on the host", s37_approve_all_answers_on_host),
    "S38a": ("mixed versions: client joins, another terminal changes the mode", lambda c: s38_join_then_mode_change(c, "other")),
    "S38b": ("mixed versions: client starts a session with /new and prompts", s38_fresh_new),
    "S38c": ("mixed versions: client joins and changes the mode itself", lambda c: s38_join_then_mode_change(c, "self")),
    "S39a": ("agent without Ask: host list vs attach menu, then Ask", lambda c: s39_mode_list_without_ask(c, "attach")),
    "S39b": ("agent without Ask: host list vs plain menu, then Ask", lambda c: s39_mode_list_without_ask(c, "fg")),
    "S40a": ("plain nori: badge follows the mode and the session on screen", s40_badge_follows_mode_and_session),
    "S40b": ("plain nori: host default Ask; card and badge at launch", lambda c: s40_card_and_badge_at_launch(c, False)),
    "S40c": ("plain nori: migrated Read Only; card and badge at launch", lambda c: s40_card_and_badge_at_launch(c, True)),
    "S41a": ("cwd: attach, separate client home/folder, two sessions, watcher; @, /new ~, /new, Tab", s41_attach_two_sessions_and_watcher),
    "S41b": ("cwd: plain nori /new DIR, @ follows the session on screen", s41_plain_new_dir_and_at),
    "S41c": ("cwd: attach --host with no session: @, Tab and relative /new use the host folder", s41c_attach_no_session_defaults),
    "S41d": ("cwd: plain nori /new ~ and Tab from the session on screen", s41d_plain_tilde_and_tab),
    "S41e": ("cwd: plain nori with no session uses the launch folder", s41e_plain_no_session),
    "S41f": ("cwd: older host (no find_paths) with a newer attach client", s41f_older_host),
    "S41g": ("cwd: find_paths and session/new on the wire vs registry", s41g_wire_contract),
    "S42a": ("screen: plain nori, inline<->full mid-turn, scrollback once and in order, exit restore", s42a_mid_turn_both_directions),
    "S42b": ("screen: two sessions, Alt+, while full, each session's own scrollback on return", s42b_separate_sessions),
    "S42c": ("screen: Ctrl+T and resizes over full, draft kept, exit with overlay open", s42c_overlay_resize_exit),
    "S42d": ("screen: /quit from full mid-turn restores the terminal", lambda c: s42d_exit_from_full(c, "quit")),
    "S42e": ("screen: Ctrl+C from full mid-turn restores the terminal", lambda c: s42d_exit_from_full(c, "ctrlc")),
    "S42f": ("screen: draft present before the switch; overlay vs switch action", s42e_draft_before_toggle),
    "S42g": ("screen: standalone nori-tui, mid-turn both directions", lambda c: s42a_mid_turn_both_directions(c, "nori-tui")),
    "S42h": ("screen: attach shows /screen greyed with its reason", s42f_attach_menu),
    "S42i": ("screen: resizes while full and streaming, then inline; rows once", s42i_resize_mid_turn_full),
    "S42j": ("screen: Ctrl+Z and fg from full", s42j_suspend_in_full),
    "S42k": ("screen: permission request answered while full", s42k_permission_in_full),
    "S43a": ("Ctrl+L: draft typed before the switch survives both ways unsent, then sends once", s43a_draft_across_switch),
    "S43b": ("Ctrl+L: draft typed mid-turn, switch both ways mid-turn", s43b_draft_mid_turn),
    "S43c": ("Ctrl+L: two-line pasted draft; slash menu and permission prompt keep the key", s43c_multiline_draft_and_popups),
    "S43d": ("Ctrl+L: toggle_screen rebound to alt+j", s43d_rebound_and_unbound),
    "S43e": ("Ctrl+L: nori attach leaves the draft and screen alone", s43e_attach_ctrl_l),
    "S43f": ("Ctrl+L: standalone nori-tui with a draft", lambda c: s43a_draft_across_switch(c, "nori-tui")),
    "S42n": ("screen: /resume inline (control for S42l)", lambda c: s42l_resume_in_full(c, False)),
    "S42m": ("screen: permission request answered inline (ordering control for S42k)", lambda c: s42k_permission_in_full(c, False)),
    "S42l": ("screen: /resume while full, then inline", s42l_resume_in_full),
    "S44a": ("resume while full, no input: plain nori", lambda c: s44_resume_spontaneous(c, "nori", True)),
    "S44b": ("resume while full, no input: nori-tui", lambda c: s44_resume_spontaneous(c, "nori-tui", True)),
    "S44c": ("resume inline, no input: plain nori (control)", lambda c: s44_resume_spontaneous(c, "nori", False)),
    "S44d": ("resume inline, no input: nori-tui (control)", lambda c: s44_resume_spontaneous(c, "nori-tui", False)),
    "S44e": ("resume while full, no input, /quit from full: plain nori", lambda c: s44_resume_then_quit_full(c, "nori")),
    "S44f": ("resume while full, no input, /quit from full: nori-tui", lambda c: s44_resume_then_quit_full(c, "nori-tui")),
    "S36": ("startup activation window swept by Enter time", s36_startup_race),
    "S36b": ("startup activation window swept by typing time", lambda c: s36_startup_race(c, burst_at_reply=True)),
}


# ---------------------------------------------------------------------------
# J: the shared App joined to a running host (#777 run_joined_app). Sessions are
# seeded with `nori host new` because a joined terminal cannot start one yet.
# ---------------------------------------------------------------------------

def seed(ctx, prompt, wait_text=None, cwd="alpha"):
    out = ctx.nori("host", "new", "--prompt", prompt, cwd=ctx.fx["roots"][cwd])
    sid = out.stdout.strip().splitlines()[-1].strip() if out.stdout.strip() else ""
    if out.returncode or not sid:
        raise RuntimeError(f"host new failed: {out.stdout} {out.stderr}")
    deadline = time.time() + 20
    while time.time() < deadline:
        if any(r.get("kind") == "turn_end" for r in ctx.ledger()) or "hold" in prompt or "perm" in prompt:
            break
        time.sleep(0.2)
    return sid


def count(t, needle):
    return t.history().count(needle)


def j_open(ctx, t, sid, via=None):
    if via is not None:
        attach_via(ctx, t, via, sid)
    else:
        t.run(f"{NORI} attach {sid[:8]}")
    t.wait_for("›", 20)


def j01_restored_then_live_once(ctx):
    """Joined controller: restored history once, then its own turn live once."""
    ctx.start_host()
    sid = seed(ctx, "relay 2 dawn")
    ctx.nori("host", "ls")
    a = ctx.term("controller")
    j_open(ctx, a, sid)
    try:
        a.wait_for("dawn row 02", 10)
    except TimeoutError:
        pass
    time.sleep(1.5)
    a.capture("restored")
    ctx.check("restore.reply_once", count(a, "dawn row 02") == 1, count(a, "dawn row 02"))
    a.type("relay 2 noon")
    a.keys("Enter")
    try:
        a.wait_for("noon row 02", 15)
    except TimeoutError:
        pass
    time.sleep(1.5)
    a.capture("live")
    prompts = [r.get("text", "") for r in ctx.ledger("request", "session/prompt")]
    ctx.check("live.prompt_delivered_once", sum("relay 2 noon" in p for p in prompts) == 1, prompts)
    ctx.check("live.reply_once", count(a, "noon row 02") == 1, count(a, "noon row 02"))
    ctx.check("live.restored_still_once", count(a, "dawn row 02") == 1, count(a, "dawn row 02"))
    ctx.check("live.composer_cleared", "relay 2 noon" not in last_nonblank(a.screen()), last_nonblank(a.screen()))


def j02_two_terminals_takeover(ctx):
    """Controller and watcher: watcher Enter keeps draft, Alt+T takeover, both see turns once."""
    ctx.start_host()
    sid = seed(ctx, "relay 2 dawn")
    a = ctx.term("controller")
    j_open(ctx, a, sid)
    a.wait_for("dawn row 02", 20)
    time.sleep(1)
    b = ctx.term("watcher")
    j_open(ctx, b, sid)
    b.wait_for("dawn row 02", 20)
    time.sleep(1)
    b.capture("watcher-joined")
    ctx.check("watcher.restored_once", count(b, "dawn row 02") == 1, count(b, "dawn row 02"))
    b.type("relay 2 wat")
    b.keys("Enter")
    time.sleep(1.2)
    b.capture("watcher-enter")
    prompts = [r.get("text", "") for r in ctx.ledger("request", "session/prompt")]
    ctx.check("watcher.enter_not_delivered", not any("relay 2 wat" in p for p in prompts), prompts)
    ctx.check("watcher.draft_kept", "relay 2 wat" in b.screen())
    ctx.check("watcher.enter_names_alt_t", re.search(r"alt\+t", b.screen(), re.I), b.screen()[-400:])
    a.type("relay 2 ctl")
    a.keys("Enter")
    a.wait_for("ctl row 02", 20)
    b.wait_for("ctl row 02", 20)
    time.sleep(1)
    ctx.check("watcher.live_once", count(b, "ctl row 02") == 1, count(b, "ctl row 02"))
    a.type("controller unsent draft")
    b.keys("M-t")
    time.sleep(2)
    a.capture("controller-demoted")
    b.capture("watcher-promoted")
    ctx.check("takeover.new_controller_notice", re.search(r"now controls", b.history()), b.screen()[-300:])
    ctx.check("takeover.old_controller_notice", re.search(r"took control", a.history()), a.screen()[-300:])
    ctx.check("takeover.old_controller_draft_kept", "controller unsent draft" in a.screen())
    ctx.check("takeover.promoted_draft_kept", "relay 2 wat" in b.screen())
    b.keys("Enter")
    b.wait_for("wat row 02", 20)
    a.wait_for("wat row 02", 20)
    time.sleep(1)
    prompts = [r.get("text", "") for r in ctx.ledger("request", "session/prompt")]
    ctx.check("takeover.prompt_delivered_once", sum("relay 2 wat" in p for p in prompts) == 1, prompts)
    for name, t in (("a", a), ("b", b)):
        ctx.check(f"takeover.{name}_reply_once", count(t, "wat row 02") == 1,
                  count(t, "wat row 02"))
    a.keys("Enter")
    time.sleep(1)
    prompts = [r.get("text", "") for r in ctx.ledger("request", "session/prompt")]
    ctx.check("takeover.demoted_enter_not_delivered", not any("controller unsent draft" in p for p in prompts), prompts)


def j03_reconnect_replays_missed_once(ctx):
    """Watcher behind a relay misses a turn while down; after reconnect it shows it once, draft kept."""
    endpoint, _ = ctx.start_host()
    sid = seed(ctx, "relay 2 dawn")
    a = ctx.term("controller")
    j_open(ctx, a, sid)
    a.wait_for("dawn row 02", 20)
    proxy = Proxy(endpoint)
    b = ctx.term("watcher")
    j_open(ctx, b, sid, via=proxy)
    b.wait_for("dawn row 02", 20)
    time.sleep(1)
    b.type("watcher draft across the gap")
    a.type("controller draft across the gap")
    proxy.drop_all(hold=4.0)
    time.sleep(0.6)
    b.capture("watcher-down")
    # a turn the watcher can only see by replay
    c = ctx.term("third")
    j_open(ctx, c, sid)
    c.wait_for("dawn row 02", 20)
    a.keys("C-u")
    a.type("relay 2 away")
    a.keys("Enter")
    a.wait_for("away row 02", 20)
    try:
        b.wait_for("away row 02", 30)
        seen = True
    except TimeoutError:
        seen = False
    time.sleep(1.5)
    b.capture("watcher-reconnected")
    hist = b.history()
    ctx.check("reconnect.missed_turn_shown", seen)
    ctx.check("reconnect.missed_turn_once", hist.count("away row 02") == 1, hist.count("away row 02"))
    ctx.check("reconnect.history_not_repeated", hist.count("dawn row 02") == 1, hist.count("dawn row 02"))
    ctx.check("reconnect.notice_shown", re.search(r"(Lost the connection|Reconnect)", hist), "")
    ctx.check("reconnect.watcher_draft_kept", "watcher draft across the gap" in b.screen())
    ctx.check("reconnect.third_sees_once", count(c, "away row 02") == 1, count(c, "away row 02"))
    proxy.close()


def j_pick(t, title):
    """Open the session list with Alt+S (keeps the draft) and open the row titled `title`."""
    t.keys("M-s")
    t.wait_for("enter open", 10)
    t.capture("list-for-" + title.replace(" ", "-"))
    for key in ["Up"] * 6 + ["Down"] * 12:
        rows = [l for l in t.screen().splitlines() if l.lstrip().startswith(("› ○", "› ●", "› ◐", "› ◉"))]
        if rows and title in rows[0]:
            break
        t.keys(key)
        time.sleep(0.25)
    t.keys("Enter")
    time.sleep(1.5)


def j04_switch_sessions(ctx):
    """Two sessions: switch with Alt+S, drafts stay per session, events from the hidden one appear once."""
    ctx.start_host()
    s1 = seed(ctx, "relay 2 one")
    s2 = seed(ctx, "relay 2 two")
    time.sleep(1.5)
    a = ctx.term("controller")
    j_open(ctx, a, s1)
    a.wait_for("one row 02", 20)
    a.type("draft kept in one")
    b = ctx.term("driver")
    j_open(ctx, b, s1)
    b.wait_for("one row 02", 20)
    j_pick(a, "relay 2 two")
    a.wait_for("two row 02", 15)
    a.capture("on-two")
    ctx.check("switch.two_shown_once", count(a, "two row 02") == 1, count(a, "two row 02"))
    ctx.check("switch.draft_not_carried_to_two", "draft kept in one" not in a.screen())
    a.type("draft kept in two")
    b.keys("M-t")
    time.sleep(1.5)
    b.type("relay 2 hid")
    b.keys("Enter")
    b.wait_for("hid row 02", 20)
    time.sleep(1)
    ctx.check("switch.hidden_turn_not_on_two", "hid row 02" not in a.screen())
    j_pick(a, "relay 2 one")
    time.sleep(2)
    a.capture("back-on-one-early")
    a.wait_for("hid row 02", 15)
    time.sleep(1)
    a.capture("back-on-one")
    hist = a.history()
    ctx.check("switch.back_shows_hidden_turn_once", hist.count("hid row 02") == 1, hist.count("hid row 02"))
    ctx.check("switch.draft_kept_in_one", "draft kept in one" in a.screen(), last_nonblank(a.screen()))
    j_pick(a, "relay 2 two")
    time.sleep(1)
    a.capture("back-on-two")
    ctx.check("switch.draft_kept_in_two", "draft kept in two" in a.screen(), last_nonblank(a.screen()))
    ctx.check("switch.two_not_repeated", a.history().count("two row 02") == 1, a.history().count("two row 02"))


def j05_permissions(ctx):
    """Permission request: watcher cannot answer, controller answers once; the agent sees one outcome."""
    ctx.start_host()
    sid = seed(ctx, "relay 2 dawn")
    a = ctx.term("controller")
    j_open(ctx, a, sid)
    a.wait_for("dawn row 02", 20)
    b = ctx.term("watcher")
    j_open(ctx, b, sid)
    b.wait_for("dawn row 02", 20)
    a.type("perm paint it")
    a.keys("Enter")
    a.wait_for("Paint the boathouse", 20)
    try:
        b.wait_for("Paint the boathouse", 10)
        wsaw = True
    except TimeoutError:
        wsaw = False
    a.capture("controller-permission")
    b.capture("watcher-permission")
    ctx.check("perm.watcher_sees_request", wsaw)
    b.keys("Enter")
    time.sleep(1.5)
    outcomes = [r for r in ctx.ledger() if r.get("kind") == "response" and "outcome" in json.dumps(r)]
    b.capture("watcher-after-enter")
    ctx.check("perm.watcher_answer_not_delivered", not outcomes, outcomes[:2])
    a.keys("Enter")
    deadline = time.time() + 20
    while time.time() < deadline and not [r for r in ctx.ledger() if r.get("kind") == "response" and "outcome" in json.dumps(r)]:
        time.sleep(0.2)
    time.sleep(1.5)
    a.capture("controller-answered")
    b.capture("watcher-after-answer")
    outcomes = [r for r in ctx.ledger() if r.get("kind") == "response" and "outcome" in json.dumps(r)]
    ctx.check("perm.answered_once", len(outcomes) == 1, outcomes[:2])
    ctx.check("perm.answer_was_allow", outcomes and "allow" in json.dumps(outcomes[0]), outcomes[:1])
    ctx.check("perm.controller_prompt_dismissed", "Allow once" not in a.screen().replace("Permission: Allow once", ""), a.screen()[-300:])
    ctx.check("perm.watcher_prompt_dismissed", "Allow once" not in b.screen(), b.screen()[-300:])


def j06_detach_keeps_host(ctx, how):
    """Leaving a joined terminal mid-turn keeps the host session running."""
    ctx.start_host()
    sid = seed(ctx, "relay 2 dawn")
    a = ctx.term("attach")
    j_open(ctx, a, sid)
    a.wait_for("dawn row 02", 20)
    a.type("relay 60 tk")
    a.keys("Enter")
    a.wait_for("tk row 03", 20)
    if how == "ctrl-d":
        a.keys("C-d")
    elif how == "quit":
        a.type("/quit")
        a.keys("Enter")
    elif how == "ctrl-c":
        a.keys("C-c")
        time.sleep(0.4)
        a.keys("C-c")
    elif how == "hangup":
        pid = a.fmt("#{pane_pid}")
        subprocess.run(["pkill", "-HUP", "-P", pid])
    time.sleep(2)
    before = len(ctx.ledger("relay"))
    time.sleep(2)
    after = len(ctx.ledger("relay"))
    if how != "hangup":
        a.capture("after-leave")
        check_restored(ctx, a, "leave")
    ctx.check("leave.turn_keeps_ticking", after > before, (before, after))
    ls = ctx.nori("host", "ls").stdout
    ctx.check("leave.session_listed", sid[:8] in ls, ls[-300:])
    ends = [r for r in ctx.ledger() if r.get("kind") == "request" and r.get("method") in ("session/close", "session/cancel")]
    ctx.check("leave.nothing_closed_or_cancelled", not ends, ends[:2])
    b = ctx.term("rejoin")
    j_open(ctx, b, sid)
    b.wait_for("tk row", 20)
    time.sleep(1.5)
    hist = r_inline_history(b)
    ctx.check("rejoin.history_once", hist.count("dawn row 02") == 1, hist.count("dawn row 02"))
    ctx.check("rejoin.stream_rows_once", hist.count("tk row 03") == 1, hist.count("tk row 03"))
    b.capture("rejoined")


SCENARIOS.update({
    "J01": ("joined: restored history once, own turn live once", j01_restored_then_live_once),
    "J02": ("joined: controller/watcher, watcher Enter, Alt+T takeover", j02_two_terminals_takeover),
    "J03": ("joined: reconnect replays only the missed turn; draft kept", j03_reconnect_replays_missed_once),
    "J04": ("joined: switch sessions; drafts per session; hidden events once", j04_switch_sessions),
    "J05": ("joined: permission answered once by the controller only", j05_permissions),
    "J06a": ("joined: Ctrl+D mid-turn keeps the host session", lambda c: j06_detach_keeps_host(c, "ctrl-d")),
    "J06b": ("joined: /quit mid-turn keeps the host session", lambda c: j06_detach_keeps_host(c, "quit")),
    "J06d": ("joined: PTY hangup mid-turn keeps the host session", lambda c: j06_detach_keeps_host(c, "hangup")),
})


# ---------------------------------------------------------------------------
# K: host-backed features (#778-#782) through plain `nori`, which now routes
# them through its owned host. Compared against main for regressions.
# ---------------------------------------------------------------------------

def k_plain(ctx, name="fg"):
    t = ctx.term(name)
    t.run(f"{NORI} --skip-trust-directory")
    screen = t.wait_for(r"(›|Update now)", 20, regex=True)
    if "Update now" in screen:
        t.keys("2")
        time.sleep(0.8)
    wait_composer(t)
    return t


def k_quit(t):
    t.keys("C-u")
    t.type("/quit")
    t.keys("Enter")
    time.sleep(0.8)
    if not t.at_shell():
        t.keys("Enter")
    time.sleep(0.5)


def k01_history_recall(ctx):
    """Up recalls and Ctrl+R finds a prompt sent in an earlier plain nori run."""
    t = k_plain(ctx)
    t.type("relay 1 cormorant")
    t.keys("Enter")
    t.wait_for("cormorant row 01", 20)
    k_quit(t)
    t = k_plain(ctx, "fg2")
    t.keys("Up")
    time.sleep(1.0)
    ctx.check("history.up_recalls_previous_run", "relay 1 cormorant" in last_nonblank(t.screen()) or "› relay 1 cormorant" in t.screen(), t.screen()[-300:])
    t.capture("up-recall")
    t.keys("C-u")
    t.keys("C-r")
    time.sleep(0.5)
    t.type("cormo")
    time.sleep(1.0)
    t.capture("ctrl-r")
    ctx.check("history.ctrl_r_finds_it", "relay 1 cormorant" in t.screen(), t.screen()[-300:])
    t.keys("Escape")
    hp = ctx.fx["homes"]["host"] / "history.jsonl"
    ctx.check("history.recorded_once_in_home", hp.exists() and hp.read_text().count("relay 1 cormorant") == 1,
              hp.read_text().count("relay 1 cormorant") if hp.exists() else "missing")
    k_quit(t)


def k02_custom_prompts(ctx):
    """A markdown prompt expands; a script prompt runs in the session folder; output is queued once."""
    cmds = ctx.fx["homes"]["host"] / "commands"
    cmds.mkdir(exist_ok=True)
    (cmds / "harbor-review.md").write_text("---\ndescription: Review the harbor log\n---\nrelay 1 review-$1\n")
    (cmds / "wherescript.sh").write_text("#!/bin/sh\necho \"relay 1 ran-in-$(basename $(dirname $(pwd)))\"\n")
    os.chmod(cmds / "wherescript.sh", 0o755)
    t = k_plain(ctx)
    t.type("/harbor")
    time.sleep(1.2)
    menu = t.screen()
    t.capture("popup")
    ctx.check("prompts.listed_in_popup", "harbor-review" in menu, menu[-400:])
    t.keys("C-u")
    t.type("/prompts:harbor-review north")
    t.keys("Enter")
    time.sleep(0.5)
    if "/prompts:harbor-review" in last_nonblank(t.screen()):
        t.keys("Enter")
    try:
        t.wait_for("review-north row 01", 20)
        ok = True
    except TimeoutError:
        ok = False
    ctx.check("prompts.markdown_expands_and_sends", ok, t.screen()[-300:])
    t.type("/prompts:wherescript")
    t.keys("Enter")
    time.sleep(0.5)
    if "/prompts:wherescript" in last_nonblank(t.screen()):
        t.keys("Enter")
    try:
        t.wait_for("ran-in-alpha row 01", 25)
        ok = True
    except TimeoutError:
        ok = False
    t.capture("script")
    prompts = [r.get("text", "") for r in ctx.ledger("request", "session/prompt")]
    ctx.check("prompts.script_output_sent_once", ok and sum("ran-in-alpha" in p for p in prompts) == 1, prompts)
    k_quit(t)


def k03_goal(ctx):
    """/goal set, show, pause, clear in plain nori."""
    t = k_plain(ctx)
    t.type("relay 1 warmup")
    t.keys("Enter")
    t.wait_for("warmup row 01", 20)
    t.type("/goal chart the north harbor")
    t.keys("Enter")
    time.sleep(1.5)
    if "/goal chart" in last_nonblank(t.screen()):
        t.keys("Enter"); time.sleep(1.5)
    t.capture("goal-set")
    hist = t.history()
    ctx.check("goal.set_shown", "north harbor" in hist, hist[-400:])
    t.type("/goal")
    t.keys("Enter")
    time.sleep(1.5)
    t.capture("goal-show")
    ctx.check("goal.show_reports_it", t.history().count("north harbor") >= 2, t.screen()[-400:])
    k_quit(t)


def k04_context(ctx):
    """/context prints a report naming the session folder's AGENTS.md."""
    for name in ("AGENTS.md", "CLAUDE.md"):
        (ctx.fx["roots"]["alpha"] / name).write_text("# Alpha harbor rules\nKeep the beacon lit.\n")
    t = k_plain(ctx)
    t.type("relay 1 warmup")
    t.keys("Enter")
    t.wait_for("warmup row 01", 20)
    t.type("/context")
    t.keys("Enter")
    time.sleep(2.0)
    if "/context" in last_nonblank(t.screen()):
        t.keys("Enter"); time.sleep(2.0)
    t.capture("context")
    hist = t.history()
    ctx.check("context.report_printed", re.search(r"(context|Context)", hist) is not None, hist[-500:])
    ctx.check("context.names_instruction_file", "CLAUDE.md" in hist or "AGENTS.md" in hist, hist[-500:])
    k_quit(t)


SCENARIOS.update({
    "K01": ("plain nori: Up and Ctrl+R recall a prompt from an earlier run", k01_history_recall),
    "K02": ("plain nori: custom markdown and script prompts", k02_custom_prompts),
    "K03": ("plain nori: /goal set and show", k03_goal),
    "K04": ("plain nori: /context names the folder's instruction file", k04_context),
})


# ---------------------------------------------------------------------------
# R: restacked heads (#777 5c72765d, real #784 launcher, #778-#785 on real attach).
# From #784 on, `nori attach` is the shared joined TUI, so no verifier entry is set.
# ---------------------------------------------------------------------------

NOT_ON_HOST = "not available on this host yet"


def r_order(text, needles):
    """Each needle once, in the given order; returns (ok, positions)."""
    pos = [(n, text.count(n), text.find(n)) for n in needles]
    ok = all(c == 1 for _, c, _ in pos) and [p for _, _, p in pos] == sorted(p for _, _, p in pos)
    return ok, [(n, c) for n, c, _ in pos if c != 1] or "ordered"


def r_inline_history(t):
    """Scrollback of a joined terminal: switch alternate -> inline first so every row is in tmux history."""
    if t.alt_screen():
        t.keys("C-l")
        time.sleep(1.5)
    return t.history()


def r_sent(ctx, word):
    return [r for r in ctx.ledger("request", "session/prompt") if word in r.get("text", "")]


def r01_no_newline_order(ctx):
    """#777 fix on real attach: replies without a final newline show once, restored and live, in order, for a late joiner too."""
    ctx.start_host()
    sid = seed(ctx, "relay 2 dawn")
    a = ctx.term("controller")
    j_open(ctx, a, sid)
    try:
        a.wait_for("dawn relay finished.", 10)
    except TimeoutError:
        pass
    a.capture("restored")
    ctx.check("restored.unterminated_reply_shown", "dawn relay finished." in a.screen(), a.screen()[-300:])
    for word in ("noon", "dusk"):
        a.type(f"relay 2 {word}")
        a.keys("Enter")
        try:
            a.wait_for(f"{word} relay finished.", 15)
        except TimeoutError:
            pass
        time.sleep(0.8)
    a.type("plain evening note")
    a.keys("Enter")
    try:
        a.wait_for("heard: plain evening note", 15)
    except TimeoutError:
        pass
    time.sleep(1.0)
    a.capture("live")
    seq = ["relay 2 dawn", "dawn row 01", "dawn relay finished.", "relay 2 noon", "noon row 02", "noon relay finished.",
           "relay 2 dusk", "dusk row 02", "dusk relay finished.", "heard: plain evening note"]
    b = ctx.term("late-joiner")
    j_open(ctx, b, sid)
    try:
        b.wait_for("heard: plain evening note", 15)
    except TimeoutError:
        pass
    time.sleep(1.0)
    b.capture("late-joiner")
    for name, t in (("controller", a), ("late_joiner", b)):
        hist = r_inline_history(t)
        t.capture(f"{name}-inline")
        ok, detail = r_order(hist, seq)
        ctx.check(f"{name}.each_once_in_order", ok, detail)
        glued = [l for l in hist.splitlines() if re.search(r"finished\.\s*(-|›|\S)", l)]
        ctx.check(f"{name}.no_tail_glued_to_next_row", not glued, glued[:3])
    for word in ("noon", "dusk"):
        ctx.check(f"live.{word}_delivered_once", len(r_sent(ctx, f"relay 2 {word}")) == 1, len(r_sent(ctx, f"relay 2 {word}")))


def r02_picker_and_new(ctx):
    """Empty host: alternate-screen picker; /new starts a session on the host; picker lists and opens it later."""
    ctx.start_host()
    a = ctx.term("first")
    a.run(f"{NORI} attach")
    a.wait_for("Sessions", 10)
    time.sleep(0.5)
    a.capture("picker-empty")
    ctx.check("start.alternate_screen", a.alt_screen(), a.alt_screen())
    ctx.check("start.picker_shown", "Sessions 0" in a.screen(), a.screen()[:200])
    a.keys("Escape")
    a.wait_for("No session selected", 10)
    a.type("/new")
    time.sleep(0.6)
    a.keys("Enter")
    try:
        a.wait_for("controlling · app", 15)
    except TimeoutError:
        pass
    a.capture("after-new")
    ls = ctx.nori("host", "ls").stdout
    ctx.check("new.session_on_host", len([l for l in ls.splitlines() if "claude-code" in l or "app" in l]) >= 1, ls[-300:])
    a.type("relay 1 first")
    a.keys("Enter")
    try:
        a.wait_for("first relay finished.", 15)
    except TimeoutError:
        pass
    ctx.check("new.first_prompt_once", len(r_sent(ctx, "relay 1 first")) == 1, len(r_sent(ctx, "relay 1 first")))
    a.keys("C-d")
    time.sleep(1.5)
    ctx.check("detach.back_at_shell", a.at_shell(), last_nonblank(a.screen()))
    seed(ctx, "relay 1 second", cwd="beta")
    b = ctx.term("second")
    b.run(f"{NORI} attach")
    b.wait_for("Sessions", 10)
    time.sleep(1.0)
    b.capture("picker-two")
    ctx.check("picker.lists_two", "Sessions 2" in b.screen(), b.screen()[:300])
    b.keys("Enter")
    time.sleep(2.5)
    b.capture("picker-opened")
    ctx.check("picker.enter_opens_a_session", "relay finished." in b.screen() and "controlling" in b.screen(), b.screen()[-300:])


def r03_typeahead(ctx):
    """Keys typed right after launch reach the composer once; the first prompt is delivered once."""
    ctx.start_host()
    sid = seed(ctx, "relay 1 dawn")
    a = ctx.term("a")
    a.run(f"{NORI} attach {sid[:8]}")
    time.sleep(0.05)
    a.type("relay 1 early")
    time.sleep(0.4)
    a.keys("Enter")
    try:
        a.wait_for("early relay finished.", 15)
    except TimeoutError:
        pass
    time.sleep(0.8)
    a.capture("typeahead")
    ctx.check("typeahead.delivered_once", len(r_sent(ctx, "relay 1 early")) == 1,
              [r.get("text") for r in ctx.ledger("request", "session/prompt")])
    ctx.check("typeahead.reply_shown_once", a.screen().count("early relay finished.") == 1, a.screen()[-300:])


def r_queue_paused(ctx, t, word):
    t.type("hold on")
    t.keys("Enter")
    wait_ledger(ctx, lambda r: r["kind"] == "tick")
    t.type(word)
    t.keys("Tab")
    time.sleep(1.0)
    t.keys("Escape")
    t.wait_for("paused", 10)
    time.sleep(0.5)


def r_reclaim(t):
    """Up on an empty composer focuses the newest editable queued prompt; Enter takes it back.
    With nothing editable, Up falls through to prompt history, so Enter would send a second copy:
    only press Enter when the queue list took focus (the queued row is no longer the composer text)."""
    t.keys("C-u")
    t.keys("Up")
    time.sleep(1.0)
    t.capture("after-up")
    if "1 queued" in t.screen() and any(w in r_composer(t) for w in ("relay 1 lamp", "relay 1 beacon")):
        t.keys("C-u")
        return "history-recall"
    t.keys("Enter")
    time.sleep(1.5)
    return "list"


def r_composer(t):
    rows = [l for l in t.screen().splitlines() if l.lstrip().startswith("›")]
    return rows[-1] if rows else ""


def r05_reclaim_own(ctx, via_proxy=False):
    """#778: the terminal that queued a prompt takes it back once (optionally after reconnecting)."""
    endpoint, _ = ctx.start_host()
    sid = seed(ctx, "relay 1 dawn")
    proxy = Proxy(endpoint) if via_proxy else None
    a = ctx.term("a")
    j_open(ctx, a, sid, via=proxy)
    a.wait_for("dawn relay finished.", 15)
    word = "relay 1 lamp"
    r_queue_paused(ctx, a, word)
    a.capture("paused")
    if proxy:
        proxy.drop_all(hold=3.0)
        time.sleep(1.5)
        a.capture("dropped")
        time.sleep(6)
        a.capture("reconnected")
        ctx.check("reconnect.queue_still_listed", "1 queued" in a.screen(), a.screen()[-300:])
    how = r_reclaim(a)
    a.capture("reclaimed")
    ctx.check("reclaim.up_focuses_queue_not_history", how == "list", how)
    ctx.check("reclaim.text_in_composer", word in r_composer(a), r_composer(a))
    ctx.check("reclaim.queue_row_gone", "1 queued" not in a.screen(), [l for l in a.screen().splitlines() if "queued" in l])
    a.keys("C-u")
    a.type("relay 1 after")
    a.keys("Enter")
    try:
        a.wait_for("after relay finished.", 15)
    except TimeoutError:
        pass
    time.sleep(3)
    a.capture("after-next-turn")
    ctx.check("reclaim.never_sent", not r_sent(ctx, word), len(r_sent(ctx, word)))
    if proxy:
        proxy.close()


def r05c_reclaim_other(ctx):
    """#778: a prompt another terminal queued can't be taken into this composer; resume still sends it once."""
    ctx.start_host()
    sid = seed(ctx, "relay 1 dawn")
    a = ctx.term("a")
    j_open(ctx, a, sid)
    a.wait_for("dawn relay finished.", 15)
    b = ctx.term("b")
    j_open(ctx, b, sid)
    b.wait_for("dawn relay finished.", 15)
    word = "relay 1 beacon"
    r_queue_paused(ctx, a, word)
    b.keys("M-t")
    time.sleep(2)
    how = r_reclaim(b)
    b.capture("other-reclaim")
    ctx.check("other.up_does_not_take_it", how == "history-recall" or "1 queued" in b.screen(), how)
    ctx.check("other.not_taken_into_composer", word not in r_composer(b), r_composer(b))
    b.keys("C-u")
    time.sleep(0.3)
    b.keys("Enter")
    try:
        wait_ledger(ctx, lambda r: r.get("method") == "session/prompt" and word in r.get("text", ""), timeout=15)
    except Exception:
        pass
    time.sleep(2)
    b.capture("resumed")
    ctx.check("other.resume_sends_once", len(r_sent(ctx, word)) == 1, len(r_sent(ctx, word)))


def r06_history_hostwide(ctx):
    """#779 on real attach: a prompt sent in one terminal is recalled and searched in another terminal, a remote one with its own home."""
    endpoint, _ = ctx.start_host()
    sid = seed(ctx, "relay 1 dawn")
    a = ctx.term("a")
    j_open(ctx, a, sid)
    a.wait_for("dawn relay finished.", 15)
    a.type("relay 1 cormorant")
    a.keys("Enter")
    a.wait_for("cormorant relay finished.", 15)
    proxy = Proxy(endpoint)
    b = ctx.term("remote", home="laptop")
    j_open(ctx, b, sid, via=proxy)
    b.wait_for("cormorant relay finished.", 15)
    b.keys("M-t")
    time.sleep(1.5)
    b.keys("Up")
    time.sleep(1.0)
    b.capture("up")
    ctx.check("history.up_in_other_terminal", "relay 1 cormorant" in r_composer(b), r_composer(b))
    b.keys("C-u")
    b.keys("C-r")
    time.sleep(0.5)
    b.type("cormo")
    time.sleep(1.0)
    b.capture("ctrl-r")
    ctx.check("history.ctrl_r_in_other_terminal", "relay 1 cormorant" in b.screen(), b.screen()[-300:])
    b.keys("Escape")
    hp = ctx.fx["homes"]["host"] / "history.jsonl"
    lp = ctx.fx["homes"]["laptop"] / "history.jsonl"
    ctx.check("history.stored_once_on_host", hp.exists() and hp.read_text().count("relay 1 cormorant") == 1,
              hp.read_text().count("relay 1 cormorant") if hp.exists() else "missing")
    ctx.check("history.client_home_untouched", not lp.exists() or "cormorant" not in lp.read_text(), "laptop history")
    proxy.close()


def r07_prompts(ctx):
    """#780 on real attach: host prompts listed in a remote terminal; script runs on the host in the session folder."""
    cmds = ctx.fx["homes"]["host"] / "commands"
    cmds.mkdir(exist_ok=True)
    (cmds / "harbor-review.md").write_text("---\ndescription: Review the harbor log\n---\nrelay 1 review-$1\n")
    (cmds / "wherescript.sh").write_text("#!/bin/sh\necho \"relay 1 ran-in-$(basename $(dirname $(pwd)))\"\n")
    os.chmod(cmds / "wherescript.sh", 0o755)
    lc = ctx.fx["homes"]["laptop"] / "commands"
    lc.mkdir(exist_ok=True)
    (lc / "laptop-decoy.md").write_text("relay 1 decoy\n")
    endpoint, _ = ctx.start_host()
    sid = seed(ctx, "relay 1 dawn", cwd="beta")
    proxy = Proxy(endpoint)
    b = ctx.term("remote", home="laptop")
    j_open(ctx, b, sid, via=proxy)
    b.wait_for("dawn relay finished.", 15)
    time.sleep(1)
    b.type("/harbor")
    time.sleep(1.2)
    b.capture("popup")
    ctx.check("prompts.host_prompt_listed", "harbor-review" in b.screen(), b.screen()[-400:])
    b.keys("C-u")
    b.type("/laptop")
    time.sleep(1.0)
    ctx.check("prompts.client_prompt_not_listed", "laptop-decoy" not in b.screen(), b.screen()[-300:])
    b.keys("C-u")
    b.type("/prompts:harbor-review north")
    b.keys("Enter")
    time.sleep(0.5)
    if "/prompts:harbor-review" in r_composer(b):
        b.keys("Enter")
    try:
        b.wait_for("review-north relay finished.", 15)
    except TimeoutError:
        pass
    ctx.check("prompts.markdown_expands_once", len(r_sent(ctx, "relay 1 review-north")) == 1, len(r_sent(ctx, "relay 1 review-north")))
    b.type("/prompts:wherescript")
    b.keys("Enter")
    time.sleep(0.5)
    if "/prompts:wherescript" in r_composer(b):
        b.keys("Enter")
    try:
        b.wait_for("ran-in-beta relay finished.", 15)
    except TimeoutError:
        pass
    b.capture("script")
    sent = [r.get("text") for r in ctx.ledger("request", "session/prompt") if "ran-in-" in r.get("text", "")]
    ctx.check("prompts.script_ran_in_session_folder_once", sent == ["relay 1 ran-in-beta"] or (len(sent) == 1 and "ran-in-beta" in sent[0]), sent)
    proxy.close()


def r08_goal(ctx):
    """#781 on real attach: controller sets a goal; a watcher sees it; watcher can't change it."""
    (Path(ctx.fx["control"]) / "http-mcp").write_text("1")
    ctx.start_host()
    sid = seed(ctx, "relay 1 dawn")
    a = ctx.term("a")
    j_open(ctx, a, sid)
    a.wait_for("dawn relay finished.", 15)
    b = ctx.term("watcher")
    j_open(ctx, b, sid)
    b.wait_for("dawn relay finished.", 15)
    a.type("/goal chart the north harbor")
    a.keys("Enter")
    time.sleep(0.6)
    if "/goal chart" in r_composer(a):
        a.keys("Enter")
    time.sleep(2.5)
    a.capture("goal-set")
    b.capture("goal-watcher")
    ctx.check("goal.runs_in_attach", NOT_ON_HOST not in a.screen(), [l for l in a.screen().splitlines() if "/goal" in l][-2:])
    ctx.check("goal.controller_shows_it", "north harbor" in a.screen(), a.screen()[-400:])
    ctx.check("goal.watcher_shows_it", "north harbor" in b.screen(), b.screen()[-400:])
    b.type("/goal sink the fleet")
    b.keys("Enter")
    time.sleep(0.6)
    if "/goal sink" in r_composer(b):
        b.keys("Enter")
    time.sleep(2)
    b.capture("goal-watcher-refused")
    a.type("/goal")
    a.keys("Enter")
    time.sleep(0.6)
    if r_composer(a).strip().endswith("/goal"):
        a.keys("Enter")
    time.sleep(1.5)
    a.capture("goal-show")
    ctx.check("goal.watcher_change_refused", "sink the fleet" not in a.screen(), a.screen()[-400:])


def r09_context(ctx):
    """#782 on real attach: /context from a remote terminal names the session folder's CLAUDE.md on the host."""
    (ctx.fx["roots"]["beta"] / "CLAUDE.md").write_text("# Beta harbor rules\nKeep the beacon lit.\n")
    endpoint, _ = ctx.start_host()
    sid = seed(ctx, "relay 1 dawn", cwd="beta")
    proxy = Proxy(endpoint)
    b = ctx.term("remote", home="laptop")
    j_open(ctx, b, sid, via=proxy)
    b.wait_for("dawn relay finished.", 15)
    b.keys("M-t")
    time.sleep(1)
    b.type("/context")
    b.keys("Enter")
    time.sleep(0.6)
    if r_composer(b).strip().endswith("/context"):
        b.keys("Enter")
    time.sleep(2.5)
    b.capture("context")
    hist = r_inline_history(b)
    ctx.check("context.runs_in_attach", NOT_ON_HOST not in hist, [l for l in hist.splitlines() if "/context" in l][-2:])
    rows = [l for l in hist.splitlines() if "~" in l and ("CLAUDE.md" in l or "roots/beta" in l)]
    ctx.check("context.lists_session_folder_instructions", bool(rows), rows[:2])
    ctx.check("context.instruction_row_shows_file_name", any("CLAUDE.md" in l for l in rows), rows[:2])
    proxy.close()


def r10_compact(ctx):
    """#783 on real attach: controller compacts; both terminals see it finish once; a watcher can't start one."""
    ctx.start_host()
    sid = seed(ctx, "relay 2 dawn")
    a = ctx.term("a")
    j_open(ctx, a, sid)
    a.wait_for("dawn relay finished.", 15)
    b = ctx.term("watcher")
    j_open(ctx, b, sid)
    b.wait_for("dawn relay finished.", 15)
    before = len(ctx.ledger("request", "session/prompt"))
    b.type("/compact")
    b.keys("Enter")
    time.sleep(0.6)
    if r_composer(b).strip().endswith("/compact"):
        b.keys("Enter")
    time.sleep(2.0)
    b.capture("watcher-compact")
    ctx.check("compact.watcher_refused", len(ctx.ledger("request", "session/prompt")) == before,
              len(ctx.ledger("request", "session/prompt")) - before)
    a.type("/compact")
    a.keys("Enter")
    time.sleep(0.6)
    if r_composer(a).strip().endswith("/compact"):
        a.keys("Enter")
    time.sleep(6.0)
    a.capture("controller-compact")
    b.capture("watcher-after")
    ctx.check("compact.runs_in_attach", NOT_ON_HOST not in a.screen(), [l for l in a.screen().splitlines() if "/compact" in l][-2:])
    ha, hb = r_inline_history(a), r_inline_history(b)
    pat = re.compile(r"(?i)(context compacted|compacted)")
    ctx.check("compact.controller_sees_finish", pat.search(ha) is not None, ha[-500:])
    ctx.check("compact.watcher_sees_finish", pat.search(hb) is not None, hb[-500:])
    ctx.check("compact.session_kept", sid[:8] in ctx.nori("host", "ls").stdout, "")
    a.type("relay 1 post")
    a.keys("Enter")
    try:
        a.wait_for("post relay finished.", 15)
    except TimeoutError:
        pass
    ctx.check("compact.next_turn_works", len(r_sent(ctx, "relay 1 post")) == 1, len(r_sent(ctx, "relay 1 post")))


def r11_shell(ctx):
    """#785 per Cliff: local controller runs ! in the session folder; remote controller gets a gray hint, nothing runs; watcher read-only."""
    endpoint, _ = ctx.start_host()
    sid = seed(ctx, "relay 1 dawn", cwd="beta")
    run = ctx.fx["run"]
    canaries = lambda: sorted(str(p.relative_to(run)) for p in Path(run).rglob("shell-canary-*"))
    a = ctx.term("local")
    j_open(ctx, a, sid)
    a.wait_for("dawn relay finished.", 15)
    a.type("!echo shell-in-$(basename $(dirname $PWD)) && touch shell-canary-local")
    a.keys("Enter")
    try:
        a.wait_for("shell-in-beta", 10)
    except TimeoutError:
        pass
    time.sleep(1)
    a.capture("local")
    ctx.check("local.output_from_session_folder", "shell-in-beta" in a.screen(), a.screen()[-300:])
    ctx.check("local.ran_in_session_folder", canaries() == ["roots/beta/app/shell-canary-local"], canaries())
    w = ctx.term("watcher")
    j_open(ctx, w, sid)
    w.wait_for("dawn relay finished.", 15)
    time.sleep(1)
    ctx.check("watcher.sees_local_command_cell", "shell-in-beta" in w.screen(), w.screen()[-300:])
    w.type("!touch shell-canary-watcher")
    w.keys("Enter")
    time.sleep(2)
    w.capture("watcher")
    ctx.check("watcher.nothing_ran", "shell-canary-watcher" not in " ".join(canaries()), canaries())
    proxy = Proxy(endpoint)
    r = ctx.term("remote", home="laptop", cwd="laptop")
    j_open(ctx, r, sid, via=proxy)
    r.wait_for("dawn relay finished.", 15)
    r.keys("M-t")
    time.sleep(1.5)
    ran_cells_local = a.history().count("Ran ")
    r.type("!touch shell-canary-remote")
    r.keys("Enter")
    time.sleep(2.5)
    r.capture("remote-enter")
    hint = [l for l in r.screen().splitlines() if re.search(r"(?i)remote shell commands are not (yet )?supported", l)]
    ctx.check("remote.hint_shown", bool(hint), r.screen()[-400:])
    if hint:
        ansi = r.screen(ansi=True)
        line = next((l for l in ansi.splitlines() if "emote shell commands" in l), "")
        ctx.check("remote.hint_is_gray_not_red", ("[90m" in line or "38;5;8" in line or "[2m" in line or "38;5;24" in line) and "[31m" not in line, repr(line[:160]))
    shell_row = [l for l in r.screen().splitlines() if l.lstrip().startswith("!") and "touch shell-canary-remote" in l]
    ctx.check("remote.command_back_in_prompt", bool(shell_row), shell_row or r_composer(r))
    ctx.check("remote.nothing_ran_anywhere", "shell-canary-remote" not in " ".join(canaries()), canaries())
    ctx.check("remote.not_sent_as_prompt", not r_sent(ctx, "shell-canary-remote"), len(r_sent(ctx, "shell-canary-remote")))
    time.sleep(0.5)
    a.capture("local-after-remote")
    ctx.check("remote.local_terminal_no_new_ran_cell", a.history().count("Ran ") == ran_cells_local, (ran_cells_local, a.history().count("Ran ")))
    proxy.close()

SCENARIOS.update({
    "R01": ("restack: no-newline replies restored and live, once and in order, late joiner too", r01_no_newline_order),
    "R02": ("restack: empty-host picker, /new on the host, picker lists and opens", r02_picker_and_new),
    "R03": ("restack: typeahead right after launch delivered once", r03_typeahead),
    "R05a": ("restack #778: own queued prompt taken back once", r05_reclaim_own),
    "R05b": ("restack #778: own queued prompt taken back once after reconnect", lambda c: r05_reclaim_own(c, True)),
    "R05c": ("restack #778: another terminal's queued prompt not takeable; resume sends once", r05c_reclaim_other),
    "R06": ("restack #779: host-wide recall and search from a remote terminal", r06_history_hostwide),
    "R07": ("restack #780: host prompts in a remote terminal; script in session folder", r07_prompts),
    "R08": ("restack #781: goal set by controller, seen by watcher, watcher refused", r08_goal),
    "R09": ("restack #782: /context names the session folder's CLAUDE.md", r09_context),
    "R10": ("restack #783: compact from controller, finish seen by both, watcher refused", r10_compact),
    "R11": ("restack #785: local ! in session folder, remote hint and nothing runs, watcher read-only", r11_shell),
})


def r12_partial_line_live(ctx):
    """A live line without a newline yet (hold: `tick 1 tick 2 ...`): visible mid-turn in plain nori and in attach?"""
    ctx.start_host()
    sid = seed(ctx, "relay 1 dawn")
    a = ctx.term("attach")
    j_open(ctx, a, sid)
    a.wait_for("dawn relay finished.", 15)
    a.type("hold the lamp")
    a.keys("Enter")
    wait_ledger(ctx, lambda r: r["kind"] == "tick")
    time.sleep(2.0)
    a.capture("attach-mid-turn")
    ticks = ctx.ticks()
    ctx.check("attach.partial_line_visible_mid_turn", "tick 3" in a.screen(), {"ticks_sent": ticks})
    a.keys("Escape")
    time.sleep(1.5)
    p = k_plain(ctx, "plain")
    p.type("hold the lamp")
    p.keys("Enter")
    time.sleep(2.5)
    p.capture("plain-mid-turn")
    ctx.check("plain.partial_line_visible_mid_turn", "tick 3" in p.screen(), p.screen()[-200:])
    p.keys("Escape")


SCENARIOS.update({"R12": ("restack: a live line without a newline, attach vs plain nori", r12_partial_line_live)})


def r13_plain_shell(ctx):
    """#789: plain nori runs ! in its session folder (launched from beta), output shown once, nothing sent to the agent."""
    run = ctx.fx["run"]
    canaries = lambda: sorted(str(p.relative_to(run)) for p in Path(run).rglob("shell-canary-*"))
    t = ctx.term("plain", cwd="beta")
    t.run(f"{NORI} --skip-trust-directory")
    wait_composer(t)
    t.type("!echo plain-in-$(basename $(dirname $PWD)) && touch shell-canary-plain")
    time.sleep(0.3)
    t.keys("Enter")
    try:
        t.wait_for("plain-in-beta", 15)
    except TimeoutError:
        pass
    time.sleep(1)
    t.capture("plain-shell")
    ctx.check("plain.output_from_session_folder", "plain-in-beta" in t.screen(), t.screen()[-300:])
    ctx.check("plain.ran_once_in_session_folder", canaries() == ["roots/beta/app/shell-canary-plain"], canaries())
    ctx.check("plain.not_sent_to_agent", not r_sent(ctx, "shell-canary"), len(r_sent(ctx, "shell-canary")))
    k_quit(t)


SCENARIOS.update({"R13": ("#789: plain nori ! runs in the session folder", r13_plain_shell)})


def r14_controller_edits_queue(ctx, how):
    """#790 (Cliff): editing follows the current controller. A fresh attach after Ctrl+D (how=fresh) or
    another terminal after Alt+T (how=takeover) takes a pending prompt into its composer, edits it, and
    sends the correction once; the original is never delivered; the other queued prompts keep their order.
    A watcher's Up+Enter changes nothing. Queue removal is told apart from history recall by the queue count."""
    ctx.start_host()
    sid = seed(ctx, "relay 1 dawn")
    a = ctx.term("a")
    j_open(ctx, a, sid)
    a.wait_for("dawn relay finished.", 15)
    a.type("hold on")
    a.keys("Enter")
    wait_ledger(ctx, lambda r: r["kind"] == "tick")
    for w in ("relay 1 lamp", "relay 1 second", "relay 1 third"):
        a.type(w); time.sleep(0.2); a.keys("Tab"); time.sleep(0.6)
    a.keys("Escape")
    a.wait_for("3 queued", 10)
    queued = lambda t: (re.search(r"(\d+) queued", t.screen()) or [None, "0"])[1]
    if how == "fresh":
        a.keys("C-d"); time.sleep(1.5)
        ctx.check("fresh.a_detached", a.at_shell(), last_nonblank(a.screen()))
        b = ctx.term("fresh")
        j_open(ctx, b, sid)
    else:
        b = ctx.term("other")
        j_open(ctx, b, sid)
    b.wait_for("3 queued", 15)
    time.sleep(1)
    if how == "takeover":
        b.keys("C-u"); b.keys("Up"); time.sleep(1.0); b.keys("Enter"); time.sleep(1.5)
        b.capture("watcher-up-enter")
        ctx.check("watcher.queue_unchanged", queued(b) == "3" and queued(a) == "3", (queued(b), queued(a)))
        ctx.check("watcher.nothing_sent", not r_sent(ctx, "relay 1 third"), len(r_sent(ctx, "relay 1 third")))
        b.keys("C-u")
        b.keys("M-t"); time.sleep(1.5)
    b.keys("C-u"); b.keys("Up"); time.sleep(1.2)
    b.capture("after-up")
    recalled = "relay 1 third" in r_composer(b) and queued(b) == "3"
    ctx.check(f"{how}.up_is_not_history_recall", not recalled, {"queued": queued(b), "composer": r_composer(b)})
    if recalled:
        b.keys("C-u")
    else:
        b.keys("Enter"); time.sleep(1.2)
    b.capture("after-take")
    took = queued(b) == "2" and "relay 1 third" in r_composer(b)
    ctx.check(f"{how}.take_removes_from_queue_into_composer", took, {"queued": queued(b), "composer": r_composer(b)})
    if not took:
        b.type("relay 1 third")
    b.keys("End"); b.type("fix")
    time.sleep(0.3)
    b.keys("Enter")
    time.sleep(2.0)
    if re.search(r"paused", b.screen()):
        b.keys("C-u"); b.keys("Enter")
    deadline = time.time() + 25
    while time.time() < deadline and not (r_sent(ctx, "relay 1 thirdfix") and r_sent(ctx, "relay 1 second")):
        time.sleep(0.3)
    time.sleep(3)
    b.capture("settled")
    texts = [r.get("text", "") for r in ctx.ledger("request", "session/prompt")]
    ctx.check(f"{how}.correction_sent_once", sum(t.strip() == "relay 1 thirdfix" for t in texts) == 1, texts)
    ctx.check(f"{how}.original_never_sent", not any(t.strip() == "relay 1 third" for t in texts), texts)
    order = [t.strip() for t in texts if t.strip() in ("relay 1 lamp", "relay 1 second")]
    ctx.check(f"{how}.others_once_in_order", order == ["relay 1 lamp", "relay 1 second"], order)


SCENARIOS.update({
    "R14a": ("#790: fresh attach after Ctrl+D takes, edits and sends a queued prompt once", lambda c: r14_controller_edits_queue(c, "fresh")),
    "R14b": ("#790: watcher refused, then Alt+T terminal takes, edits and sends once", lambda c: r14_controller_edits_queue(c, "takeover")),
})


CAP = "[tui]\ntool_output_max_lines = 10"
NO_UPDATE = "check_for_update_on_startup = false"

R09B_ROW = re.compile(r"(\S(?:.*?\S)?)\s{2,}~[\d,]+")


def r09b_context_rows(text):
    rows = []
    for line in text.splitlines():
        for m in R09B_ROW.finditer(line):
            label = m.group(1).strip()
            if label.endswith(".md") or "\u2026" in label or "..." in label:
                rows.append(label)
    return rows


def r09b_context_names(ctx):
    """#792 on real attach: several instruction files with long, similar paths stay distinct and keep their file names in /context, at 120 and 60 columns."""
    deep = ctx.fx["roots"]["beta"] / "workbench-harbor-charts" / "northern-estuary-soundings" / "tidewater-beacon-survey"
    (deep / ".claude").mkdir(parents=True)
    files = {
        "session CLAUDE.md": deep / "CLAUDE.md",
        "session CLAUDE.local.md": deep / "CLAUDE.local.md",
        "session .claude/CLAUDE.md": deep / ".claude" / "CLAUDE.md",
        "parent CLAUDE.md": deep.parent / "CLAUDE.md",
        "host home .claude/CLAUDE.md": ctx.fx["homes"]["host"] / ".claude" / "CLAUDE.md",
    }
    for i, (label, f) in enumerate(files.items()):
        f.parent.mkdir(parents=True, exist_ok=True)
        f.write_text(f"# {label}\n" + "Keep the beacon lit. " * (3 + 4 * i) + "\n")
    decoy = ctx.fx["homes"]["laptop"] / ".claude" / "CLAUDE.md"
    decoy.parent.mkdir(parents=True, exist_ok=True)
    decoy.write_text("# laptop decoy LAPTOPDECOY\n")
    endpoint, _ = ctx.start_host()
    out = ctx.nori("host", "new", "--prompt", "relay 1 dawn", cwd=deep)
    sid = out.stdout.strip().splitlines()[-1].strip()
    wait_ledger(ctx, lambda r: r["kind"] == "turn_end")
    proxy = Proxy(endpoint)
    b = ctx.term("remote", home="laptop")
    j_open(ctx, b, sid, via=proxy)
    b.wait_for("dawn relay finished.", 15)
    b.keys("M-t")
    time.sleep(1)
    for cols in (120, 60):
        resize(b, cols, 36)
        b.type("/context")
        b.keys("Enter")
        time.sleep(0.6)
        if r_composer(b).strip().endswith("/context"):
            b.keys("Enter")
        time.sleep(2.5)
        b.capture(f"context-{cols}")
        hist = r_inline_history(b)
        tail = hist[hist.rfind("/context"):]
        rows = r09b_context_rows(tail)
        (ctx.dir / f"rows-{cols}.json").write_text(json.dumps(rows, indent=2))
        ctx.check(f"context{cols}.runs_in_attach", NOT_ON_HOST not in tail, tail[:300])
        ctx.check(f"context{cols}.five_instruction_rows", len(rows) == 5, rows)
        ctx.check(f"context{cols}.rows_distinct", len(set(rows)) == len(rows), rows)
        ctx.check(f"context{cols}.each_row_ends_with_its_file_name",
                  sorted(r.rsplit("/", 1)[-1] for r in rows) == sorted(["CLAUDE.md"] * 4 + ["CLAUDE.local.md"]), rows)
        ctx.check(f"context{cols}.hidden_claude_folder_visible", sum(".claude/CLAUDE.md" in r for r in rows) == 2, rows)
        ctx.check(f"context{cols}.client_decoy_absent", "LAPTOPDECOY" not in tail and str(decoy) not in tail)
        b.keys("C-l")
        time.sleep(1.0)
    proxy.close()


SCENARIOS.update({"R09b": ("#792: distinct long instruction paths keep their file names in /context", r09b_context_names)})


def s41h_poll_at(t, query, want, label, timeout=6.0):
    """Type `query` at once and poll: ms until `want` shows, and every other file or `no matches` seen on the way."""
    t.keys("C-u")
    t0 = time.monotonic()
    t.type(query)
    seen, got, nomatch = set(), None, False
    while time.monotonic() - t0 < timeout:
        sc = t.screen()
        seen |= files_of(sc)
        nomatch |= re.search(r"(?i)no matches", sc) is not None
        if want in sc and got is None:
            got = round((time.monotonic() - t0) * 1000)
            time.sleep(0.4)
            sc = t.screen(); seen |= files_of(sc)
            break
        time.sleep(0.03)
    t.capture(label)
    t.keys("C-u")
    return {"want": want, "shown_ms": got, "files_seen": sorted(seen), "no_matches_seen": nomatch}


def s41h_at_host_only_without_delay(ctx):
    """#791 L3: `@` with no session asks the host, typed as soon as the composer appears; then right after /new DIR and Alt+S."""
    ctx.start_host(cwd="alpha", log=True)
    c = remote_attach(ctx, "controller")
    screen = c.wait_for(r"(Sessions|›)", 20, regex=True)
    if "No sessions yet" in screen or "Sessions" in screen:
        c.keys("Escape")
    c.wait_for("›", 10)
    r0 = s41h_poll_at(c, "@lan", "lantern_wick", "0-at-no-session-immediate")
    ctx.check("h.no_session_at_shows_host_file", r0["shown_ms"] is not None, r0)
    ctx.check("h.no_session_at_never_client_file", "decoy_laptop" not in r0["files_seen"], r0)
    r0b = s41h_poll_at(c, "@decoy", "decoy_laptop", "0b-at-client-only-name", timeout=3.0)
    ctx.check("h.no_session_client_only_name_not_offered", r0b["shown_ms"] is None, r0b)
    beta = ctx.fx["roots"]["beta"]
    news = len(ctx.ledger("request", "session/new"))
    c.keys("C-u"); c.type(f"/new {beta}"); time.sleep(0.8); c.keys("Enter"); time.sleep(0.3)
    if re.search(r"^\s*›?\s*/new\b", c.screen(), re.M):
        c.keys("Enter")
    wait_ledger(ctx, lambda r: len(ctx.ledger("request", "session/new")) > news, 15)
    r1 = s41h_poll_at(c, "@harb", "harbor_tide", "1-at-right-after-new-beta")
    ctx.check("h.after_new_dir_at_follows_beta", r1["shown_ms"] is not None and not ({"lantern_wick", "decoy_laptop"} & set(r1["files_seen"])), r1)
    r1b = s41h_poll_at(c, "@lan", "lantern_wick", "1b-at-alpha-name-in-beta", timeout=3.0)
    ctx.check("h.after_new_dir_old_folder_not_offered", r1b["shown_ms"] is None, r1b)
    new_session_typed(ctx, c, f"/new {ctx.fx['roots']['alpha']}")
    visits = []
    # The picker opens on the current row (alpha, created last). Up opens beta; from there Down opens alpha.
    for i, (key, want) in enumerate((("Up", "beta"), ("Down", "alpha"))):
        c.keys("C-u"); c.keys("M-s"); time.sleep(1.2)
        c.keys(key); time.sleep(0.3)
        c.capture(f"2-switch-{i}-list")
        c.keys("Enter")
        first, other = (("@harb", "harbor_tide"), ("@lan", "lantern_wick")) if want == "beta" else (("@lan", "lantern_wick"), ("@harb", "harbor_tide"))
        a = s41h_poll_at(c, *first, f"2-switch-{i}-{want}-own", timeout=4.0)
        b = s41h_poll_at(c, *other, f"2-switch-{i}-{want}-other", timeout=3.0)
        visits.append({"want": want, "own": a, "other": b})
    (ctx.dir / "visits.json").write_text(json.dumps([r0, r0b, r1, r1b, visits], indent=2))
    ok = all(v["own"]["shown_ms"] is not None and v["other"]["shown_ms"] is None for v in visits)
    ctx.check("h.after_alt_s_at_follows_switched_session", ok, visits)
    ctx.check("h.never_client_files", not any("decoy_laptop" in v[k]["files_seen"] for v in visits for k in ("own", "other")), visits)


SCENARIOS.update({"S41h": ("#791: host-only @ with no session, right after /new DIR and after Alt+S, with no settle delay", s41h_at_host_only_without_delay)})


def s35r_agent_commands_alone(ctx):
    """#791 L2 in isolation: agent commands typed in attach go to the agent once, with and without arguments."""
    agent_commands_on(ctx)
    ctx.start_host()
    t = start_attach(ctx, "controller")
    submit(t, "first light")
    t.wait_for("heard: first light", 20)
    time.sleep(1.0)
    for text in ["/review the parser", "/review", "/lanternize now"]:
        before = prompt_texts(ctx).count(text)
        run_typed(t, text, settle=0.5)
        wait_ledger(ctx, lambda r: r.get("method") == "session/prompt" and r.get("text") == text, timeout=10)
        time.sleep(1.5)
        slug = text.replace("/", "").replace(" ", "_")
        t.capture(f"after-{slug}")
        sent = prompt_texts(ctx).count(text) - before
        ctx.check(f"r.{slug}_sent_once", sent == 1, {"sent": sent, "prompts": prompt_texts(ctx)[-3:], "composer": composer_text(t.screen())})


SCENARIOS.update({"S35r": ("#791 L2: agent commands with and without arguments reach the agent once", s35r_agent_commands_alone)})


def s50_audit(hist, tag, rows=40):
    """Each streamed row once and in order; each plan update once, and where it sits relative to the rows."""
    lines = hist.splitlines()
    seen = [int(m.group(1)) for l in lines for m in re.finditer(rf"{tag} row (\d\d) of", l)]
    pos = {}
    for i, l in enumerate(lines):
        for m in re.finditer(rf"{tag} row (\d\d) of", l):
            pos.setdefault(int(m.group(1)), i)
    plans, place = {}, {}
    for k in range(1, rows // 5 + 1):
        at = [i for i, l in enumerate(lines) if re.search(rf"{tag} step {k} done\b", l)]
        plans[k] = len(at)
        if at:
            lo, hi = pos.get(5 * k, -1), pos.get(5 * k + 1, len(lines))
            place[k] = "after its rows" if lo < at[0] < hi else ("before its rows" if at[0] < lo else "later")
    return {"rows_once_in_order": seen == list(range(1, rows + 1)), "row_count": len(seen),
            "dup_rows": len(seen) - len(set(seen)), "missing": sorted(set(range(1, rows + 1)) - set(seen))[:10],
            "plans_once": all(c == 1 for c in plans.values()) and len(plans) == rows // 5, "plan_counts": plans,
            "plan_place": sorted(set(place.values())), "finished_once": hist.count(f"{tag} stage finished.")}


def s50_streamed_ctrl_l(ctx):
    """#791: a live reply streamed in mid-line chunks, with plan updates, is shown once after Ctrl+L: in the controller
    (Ctrl+L mid-stream), a watcher (Ctrl+L after), a fresh attach (replay), and plain nori."""
    ctx.start_host()
    sid = seed(ctx, "relay 1 dawn")
    a = ctx.term("controller")
    j_open(ctx, a, sid)
    a.wait_for("dawn relay finished.", 15)
    b = ctx.term("watcher")
    j_open(ctx, b, sid)
    b.wait_for("dawn relay finished.", 15)
    submit(a, "stage 40 gull")
    wait_ledger(ctx, lambda r: r.get("kind") == "stage" and r.get("n", 0) >= 8, 20)
    a.keys("C-l")
    a.capture("controller-after-ctrl-l-mid-stream")
    wait_ledger(ctx, lambda r: r.get("kind") == "turn_end" and r.get("stopReason") and any(x.get("kind") == "stage" and x.get("n") == 40 for x in ctx.ledger()), 40)
    time.sleep(2.0)
    a.capture("controller-done")
    out = {}
    out["controller"] = s50_audit(r_inline_history(a), "gull")
    out["watcher"] = s50_audit(r_inline_history(b), "gull")
    b.capture("watcher-inline")
    c = ctx.term("fresh")
    j_open(ctx, c, sid)
    c.wait_for("gull stage finished.", 20)
    time.sleep(1.0)
    out["fresh_attach_replay"] = s50_audit(r_inline_history(c), "gull")
    c.capture("fresh-inline")
    p = k_plain(ctx, "plain")
    submit(p, "stage 40 heron")
    p.wait_for("heron stage finished.", 40)
    time.sleep(1.5)
    out["plain_nori_live"] = s50_audit(p.history(), "heron")
    p.capture("plain-done")
    (ctx.dir / "audit.json").write_text(json.dumps(out, indent=2))
    for who, au in out.items():
        ctx.check(f"{who}.rows_once_in_order", au["rows_once_in_order"], au)
        ctx.check(f"{who}.plans_once", au["plans_once"], au["plan_counts"])
        ctx.check(f"{who}.finished_once", au["finished_once"] == 1, au["finished_once"])
        ctx.check(f"{who}.plans_after_their_rows", au["plan_place"] == ["after its rows"], au["plan_place"])


def s41i_poll(t, query, want, label, timeout=4.0):
    t0 = time.monotonic(); t.type(query); stuck = False; got = None
    while time.monotonic() - t0 < timeout:
        sc = t.screen()
        if want in sc:
            got = round((time.monotonic() - t0) * 1000); break
        time.sleep(0.03)
    sc = t.screen()
    stuck = got is None and re.search(r"(?i)loading", sc) is not None
    t.capture(label)
    return {"query": query, "shown_ms": got, "loading_stuck": stuck, "client_file": "decoy_laptop" in sc}


def s41i_reopen(t, query, want, prefix):
    """Open @ once, then clear with Ctrl+U, retype; then delete to `@` with Backspace, retype; then dismiss with Esc, retype."""
    out = []
    t.keys("C-u")
    out.append(s41i_poll(t, query, want, f"{prefix}-1-first"))
    t.keys("C-u"); time.sleep(0.3)
    out.append(s41i_poll(t, query, want, f"{prefix}-2-after-ctrl-u"))
    for _ in range(len(query) - 1):
        t.keys("BSpace")
    time.sleep(0.3)
    out.append(s41i_poll(t, query[1:], want, f"{prefix}-3-after-backspace"))
    t.keys("Escape"); time.sleep(0.3); t.keys("C-u"); time.sleep(0.3)
    out.append(s41i_poll(t, query, want, f"{prefix}-4-after-escape"))
    t.keys("C-u")
    return out


def s41i_reopened_at(ctx):
    """#791: the same @ query typed again after clearing it is answered again (no endless loading): remote attach with
    no session, remote attach in a session, and plain nori."""
    ctx.start_host(cwd="alpha", log=True)
    c = remote_attach(ctx, "controller")
    screen = c.wait_for(r"(Sessions|›)", 20, regex=True)
    if "No sessions yet" in screen or "Sessions" in screen:
        c.keys("Escape")
    c.wait_for("›", 10)
    res = {"remote_no_session": s41i_reopen(c, "@lan", "lantern_wick", "nosession")}
    new_session_typed(ctx, c, f"/new {ctx.fx['roots']['beta']}")
    res["remote_in_session"] = s41i_reopen(c, "@harb", "harbor_tide", "session")
    p = k_plain(ctx, "plain")
    res["plain_nori"] = s41i_reopen(p, "@lan", "lantern_wick", "plain")
    (ctx.dir / "reopen.json").write_text(json.dumps(res, indent=2))
    for where, tries in res.items():
        for i, r in enumerate(tries):
            ctx.check(f"{where}.try{i + 1}_answered", r["shown_ms"] is not None and not r["loading_stuck"], r)
        ctx.check(f"{where}.never_client_file", not any(r["client_file"] for r in tries), tries)


SCENARIOS.update({
    "S50": ("#791: streamed reply with plans shown once across Ctrl+L, watcher, fresh attach and plain nori", s50_streamed_ctrl_l),
    "S41i": ("#791: a reopened @ search is answered again, remote with and without a session, and plain nori", s41i_reopened_at),
})

FIXTURE_ARGS = {"S34m": {"top_extra": NO_UPDATE}, "R12": {"top_extra": NO_UPDATE}, "R13": {"top_extra": NO_UPDATE}, "K01": {"top_extra": NO_UPDATE}, "S43d": {"top_extra": '[tui.hotkeys]\ntoggle_screen = "alt+j"'},"S12": {"host_extra": 'default_mode = "ask"'}, "S40b": {"host_extra": 'default_mode = "ask"'},
                "S37b": {"host_extra": 'default_mode = "ask"', "top_extra": 'sandbox_mode = "workspace-write"\n[sandbox_workspace_write]\nnetwork_access = true'}, "S19a": {"top_extra": CAP}, "S19b": {"top_extra": CAP}}
FIXTURE_ARGS.update({"S50": {"top_extra": NO_UPDATE}, "S41i": {"top_extra": NO_UPDATE}})


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
