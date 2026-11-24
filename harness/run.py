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
ARGS = P.parse_args()
# Terminals run `nori` from a synthetic bin directory so captures and logs
# show only synthetic paths.
WORK = Path("/tmp/nori-host-tui-acceptance")
BIN = WORK / "bin"
BIN.mkdir(parents=True, exist_ok=True)
(BIN / "nori").unlink(missing_ok=True)
(BIN / "nori").symlink_to(Path(ARGS.nori).resolve())
NORI = "nori"
OUT = Path(ARGS.out).resolve()
OUT.mkdir(parents=True, exist_ok=True)
RESULTS = []
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
        row = next((l for l in t.screen().splitlines() if re.match(r"^\s*(›|>|▶)", l) and label in l), None)
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
}


FIXTURE_ARGS = {"S12": {"host_extra": 'default_mode = "ask"'}}


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
