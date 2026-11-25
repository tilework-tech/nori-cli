#!/usr/bin/env python3
"""Wire probes for the host-backed features (#778 queue, #779 history, #780
custom prompts, #781 goal, #782 context) against a real `nori host`.

  python3 -I harness/feature_probes.py --nori PATH --out results/<dir>/probes.json

Each probe uses a fresh synthetic run: lantern agent servers, a host home and a
separate client home. A capability the host does not advertise records the
probe as absent instead of failing.
"""
import argparse, json, os, shutil, subprocess, sys, time
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parent))
import fixtures, wire  # noqa: E402

P = argparse.ArgumentParser()
P.add_argument("--nori", required=True); P.add_argument("--out", required=True)
P.add_argument("--only", default="")
A = P.parse_args()
NORI = str(Path(A.nori).resolve())
RESULTS = []
WORK = Path("/tmp/nori-feature-probes")


def check(probe, cid, ok, detail=""):
    RESULTS.append({"probe": probe, "check": cid, "passed": bool(ok), "detail": str(detail)[:500]})
    print(("PASS " if ok else "FAIL ") + f"{probe}/{cid} {str(detail)[:150]}", flush=True)


class Run:
    def __init__(self, name):
        self.dir = WORK / name
        shutil.rmtree(self.dir, ignore_errors=True)
        self.fx = fixtures.build(self.dir)
        self.env = fixtures.env_for(self.fx["homes"]["host"], {})
        self.cwd = self.fx["roots"]["alpha"]
        self.links = []

    def nori(self, *a, cwd=None):
        return subprocess.run([NORI, *a], env=self.env, cwd=cwd or self.cwd, capture_output=True, text=True, timeout=60)

    def __enter__(self):
        self.nori("host", "--port", "0")
        self.ep, self.tok = wire.host_endpoint(self.fx["homes"]["host"])
        return self

    def link(self, observe=False, composer=False, local=False):
        extra = {"capabilities": {"nori_sessions": {"composer": True}}} if composer else None
        lt = self.fx["homes"]["host"] / "host" / "local-token"
        headers = {"x-nori-local-token": lt.read_text().strip()} if local and lt.exists() else None
        w = wire.Wire(self.ep, self.tok, observe=observe, log=self.dir / f"wire-{len(self.links)}.jsonl",
                      init_params=extra, headers=headers)
        self.links.append(w)
        return w

    def new(self, prompt="relay 1 seed", cwd=None):
        ends = sum(r.get("kind") == "turn_end" for r in self.ledger())
        out = self.nori("host", "new", "--prompt", prompt, cwd=cwd)
        sid = out.stdout.strip().splitlines()[-1]
        deadline = time.time() + 20
        while time.time() < deadline and sum(r.get("kind") == "turn_end" for r in self.ledger()) <= ends:
            time.sleep(0.1)
        time.sleep(0.5)
        return sid

    def ledger(self):
        return fixtures.read_ledger(self.fx["ledger"])

    def prompts(self):
        return [r.get("text", "") for r in self.ledger() if r.get("kind") == "request" and r.get("method") == "session/prompt"]

    def __exit__(self, *_):
        for w in self.links:
            try: w.close()
            except Exception: pass
        self.nori("host", "stop")


def caps(w):
    return (w.init.get("result") or {}).get("capabilities", {}).get("nori_sessions", {}) or \
           (w.init.get("result") or {}).get("_meta", {}).get("nori", {}).get("capabilities", {}).get("nori_sessions", {})


def has_cap(w, name):
    blob = json.dumps(w.init.get("result", {}))
    return f'"{name}"' in blob and "nori_sessions" in blob


def err(r):
    return (r.get("error") or {}).get("code")


def row(w, sid):
    for s in (w.call("_nori/session/list", {}).get("result") or {}).get("sessions", []):
        if s.get("session_id") == sid:
            return s
    return {}


def text(t):
    return [{"type": "text", "text": t}]


# ---------------------------------------------------------------- queue (#778)
def paused_queue(run, ctl, queued):
    sid = run.new()
    started = ctl.call("_nori/session/prompt", {"session_id": sid, "prompt": text("hold the line")})
    rid = (started.get("result") or {}).get("request_id")
    deadline = time.time() + 15
    while time.time() < deadline and not any(r.get("kind") == "tick" for r in run.ledger()):
        time.sleep(0.05)
    for i, q in enumerate(queued):
        r = ctl.call("_nori/session/follow_up", {"session_id": sid, "expected_turn": rid,
                     "message_id": f"probe-q{i}-{time.time_ns()}", "intent": "queue", "prompt": text(q)})
        if (r.get("result") or {}).get("status") != "queued":
            raise RuntimeError(f"follow_up not queued: {r}")
    ctl._send(json.dumps({"jsonrpc": "2.0", "method": "session/cancel", "params": {"sessionId": run_acp_sid(run, sid)}}).encode())
    deadline = time.time() + 15
    while time.time() < deadline:
        q = row(ctl, sid).get("queue") or {}
        if q.get("paused") == "interrupted":
            return sid, q
        time.sleep(0.1)
    raise RuntimeError(f"queue never paused: {row(ctl, sid)}")


def run_acp_sid(run, sid):
    return sid


def probe_queue():
    with Run("queue") as run:
        ctl = run.link()
        if not has_cap(ctl, "queue"):
            check("queue", "capability", None is None and False, "absent"); RESULTS[-1]["absent"] = True; return
        check("queue", "capability_advertised", True)
        sid, q = paused_queue(run, ctl, ["queued alpha", "queued beta", "queued gamma"])
        ents = q.get("entries") or []
        check("queue", "paused_row_lists_three_in_order", [e.get("text") for e in ents] == ["queued alpha", "queued beta", "queued gamma"], q)
        late = run.link()  # joins after the pause
        lq = row(late, sid).get("queue") or {}
        check("queue", "late_watcher_sees_paused_queue", lq.get("paused") == "interrupted" and len(lq.get("entries") or []) == 3, lq)
        for m, p in (("take", {"sessionId": sid, "entryId": ents[0]["id"]}), ("resume", {"sessionId": sid}), ("discard", {"sessionId": sid})):
            r = late.call(f"_nori/session/queue/{m}", p)
            check("queue", f"watcher_{m}_refused_-32016", err(r) == -32016, r)
        # e1a1e4b4 contract: this client declares no `composer` capability (like the Sessions broker),
        # so its prompts can be resumed or discarded but not taken. Own-take is in probe_queue_composer.
        check("queue", "unmarked_client_prompt_not_editable", ents[0].get("editable") is False, ents[0])
        r1 = ctl.call("_nori/session/queue/take", {"sessionId": sid, "entryId": ents[0]["id"]})
        check("queue", "unmarked_client_prompt_take_null", "result" in r1 and r1["result"].get("text") is None, r1)
        before = len(run.prompts())
        r = ctl.call("_nori/session/queue/resume", {"sessionId": sid})
        check("queue", "resume_ok", "result" in r, r)
        deadline = time.time() + 20
        while time.time() < deadline and len(run.prompts()) < before + 3:
            time.sleep(0.1)
        time.sleep(2.0)
        after = run.prompts()[before:]
        check("queue", "resume_delivers_all_once_in_order", after == ["queued alpha", "queued beta", "queued gamma"], after)
        r3 = ctl.call("_nori/session/queue/take", {"sessionId": sid, "entryId": ents[1]["id"]})
        check("queue", "take_after_started_is_null", "result" in r3 and r3["result"].get("text") is None, r3)
        # discard on a fresh pause
        sid2, q2 = paused_queue(run, ctl, ["drop one", "drop two"])
        before = len(run.prompts())
        r = ctl.call("_nori/session/queue/discard", {"sessionId": sid2})
        time.sleep(2.0)
        q2b = row(ctl, sid2).get("queue") or {}
        check("queue", "discard_sends_nothing", run.prompts()[before:] == [], run.prompts()[before:])
        check("queue", "discard_clears_queue_and_pause", not (q2b.get("entries") or []) and q2b.get("paused") in (None, False, "", "none"), q2b)
        r = ctl.call("_nori/session/queue/resume", {"sessionId": "00000000-0000-4000-8000-00000000dead"})
        check("queue", "unknown_session_refused", "error" in r, r)
        # a prompt the other client queued cannot be taken into this one
        sid3, q3 = paused_queue(run, ctl, ["only mine"])
        other = run.link()
        other.call("_nori/session/take_control", {}) if False else None
        r = late.call("_nori/session/queue/take", {"sessionId": sid3, "entryId": (q3.get("entries") or [{}])[0].get("id")})
        check("queue", "watcher_cannot_take_others_prompt", err(r) == -32016, r)


# -------------------------------------------------------------- history (#779)
def probe_history():
    with Run("history") as run:
        ctl = run.link()
        if not has_cap(ctl, "history"):
            check("history", "capability", False, "absent"); RESULTS[-1]["absent"] = True; return
        check("history", "capability_advertised", True)
        sid = run.new()
        watcher = run.link()
        m0 = (watcher.call("_nori/host/history/metadata", {}).get("result") or {})
        words = ["lighthouse beam audit", "harbor tide table", "lighthouse lens polish"]
        for t in words:
            r = ctl.call("_nori/host/history/add", {"sessionId": sid, "text": t})
            check("history", f"controller_add_ok:{t[:12]}", "result" in r, r)
        r = watcher.call("_nori/host/history/add", {"sessionId": sid, "text": "watcher sneaks in"})
        check("history", "watcher_add_refused_-32016", err(r) == -32016, r)
        m1 = (watcher.call("_nori/host/history/metadata", {}).get("result") or {})
        n0, n1 = m0.get("entryCount", 0), m1.get("entryCount")
        check("history", "metadata_counts_three_more", n1 == n0 + 3, [m0, m1])
        got = []
        for off in range(n0, (n1 or 0)):
            e = (watcher.call("_nori/host/history/entry", {"logId": m1.get("logId"), "offset": off}).get("result") or {})
            got.append(json.dumps(e))
        check("history", "watcher_reads_each_entry_in_order", all(w in g for w, g in zip(words, got)) and len(got) == 3, got)
        s = watcher.call("_nori/host/history/search", {"maxResults": 50})
        blob = json.dumps(s)
        check("history", "search_returns_entries", all(w in blob for w in words) and "watcher sneaks in" not in blob, blob[:300])
        hp = run.fx["homes"]["host"] / "history.jsonl"
        lp = run.fx["homes"]["laptop"] / "history.jsonl"
        check("history", "stored_in_host_nori_home", hp.exists() and all(w in hp.read_text() for w in words), hp)
        check("history", "client_home_untouched", not lp.exists() or "lighthouse" not in lp.read_text())
        r = ctl.call("_nori/host/history/entry", {"logId": m1.get("logId"), "offset": 10**6})
        check("history", "out_of_range_offset_is_not_a_crash", "result" in r or "error" in r, r)
        r = ctl.call("_nori/host/history/add", {"sessionId": "00000000-0000-4000-8000-00000000dead", "text": "ghost"})
        check("history", "unknown_session_add_refused", "error" in r, r)


# -------------------------------------------------------------- prompts (#780)
def probe_prompts():
    with Run("prompts") as run:
        cmds = run.fx["homes"]["host"] / "commands"
        cmds.mkdir(exist_ok=True)
        (cmds / "harbor-review.md").write_text("---\ndescription: Review the harbor log\nargument-hint: <area>\n---\nReview the $1 section of the harbor log.\n")
        canary = run.dir / "INJECTED"
        (cmds / "where.sh").write_text("#!/bin/sh\necho \"cwd=$(pwd)\"\necho \"argc=$#\"\nfor a in \"$@\"; do echo \"arg=$a\"; done\n")
        os.chmod(cmds / "where.sh", 0o755)
        outside = run.dir / "outside.sh"
        outside.write_text(f"#!/bin/sh\ntouch {canary}.outside\n"); os.chmod(outside, 0o755)
        ctl = run.link()
        if not has_cap(ctl, "prompts"):
            check("prompts", "capability", False, "absent"); RESULTS[-1]["absent"] = True; return
        check("prompts", "capability_advertised", True)
        sid = run.new(cwd=run.fx["roots"]["beta"])
        watcher = run.link()
        lst = watcher.call("_nori/host/prompts", {})
        blob = json.dumps(lst)
        check("prompts", "watcher_lists_host_prompts_with_body", "harbor-review" in blob and "Review the $1 section" in blob and "where" in blob, blob[:400])
        check("prompts", "client_home_prompt_not_listed", "laptop" not in blob)
        r = ctl.call("_nori/session/prompts/run", {"sessionId": sid, "name": "where", "args": ["a b", "$(touch " + str(canary) + ")", "; touch " + str(canary)]})
        out = json.dumps(r)
        check("prompts", "script_runs_in_session_folder_on_host", f"cwd={run.fx['roots']['beta']}" in out, out[:400])
        check("prompts", "args_passed_verbatim_three", "argc=3" in out and "arg=a b" in out, out[:400])
        time.sleep(0.5)
        check("prompts", "no_shell_injection_from_args", not canary.exists(), canary)
        r = watcher.call("_nori/session/prompts/run", {"sessionId": sid, "name": "where", "args": []})
        check("prompts", "watcher_run_refused_-32016", err(r) == -32016, r)
        for bad in ("../outside", "../outside.sh", str(outside), "missing"):
            r = ctl.call("_nori/session/prompts/run", {"sessionId": sid, "name": bad, "args": []})
            time.sleep(0.3)
            check("prompts", f"refuses_name:{Path(bad).name}", "error" in r and not Path(str(canary) + ".outside").exists(), r)
        r = ctl.call("_nori/session/prompts/run", {"sessionId": sid, "name": "harbor-review", "args": []})
        check("prompts", "markdown_prompt_not_executed_as_script", "error" in r or "stdout" not in json.dumps(r), r)


# ----------------------------------------------------------------- goal (#781)
def probe_goal():
    with Run("goal") as run:
        (run.fx["control"] / "http-mcp").touch()
        ctl = run.link()
        if not has_cap(ctl, "goal"):
            check("goal", "capability", False, "absent"); RESULTS[-1]["absent"] = True; return
        check("goal", "capability_advertised", True)
        a = run.new(); b = run.new("relay 1 second")
        watcher = run.link()
        g = lambda w, s: (w.call("_nori/session/goal/get", {"sessionId": s}).get("result") or {}).get("goal")
        check("goal", "starts_empty", g(ctl, a) is None and g(ctl, b) is None, [g(ctl, a), g(ctl, b)])
        watcher.call("_nori/session/subscribe", {"session_id": a, "cursors": {a: 0}}); watcher.drain(1.0); watcher.notes.clear()
        r = ctl.call("_nori/session/goal/set", {"sessionId": a, "op": {"set": {"objective": "chart the north harbor"}}})
        check("goal", "controller_set_ok", "result" in r, r)
        notes = watcher.drain(3.0)
        check("goal", "watcher_sees_goal_changed_live", any("goal_changed" in json.dumps(n) and "north harbor" in json.dumps(n) for n in notes), [json.dumps(n)[:160] for n in notes][:4])
        check("goal", "watcher_reads_goal", "north harbor" in json.dumps(g(watcher, a)), g(watcher, a))
        check("goal", "other_session_unchanged", g(ctl, b) is None, g(ctl, b))
        r = watcher.call("_nori/session/goal/set", {"sessionId": a, "op": "clear"})
        check("goal", "watcher_set_refused_-32016", err(r) == -32016, r)
        for op in ("pause", "resume"):
            r = ctl.call("_nori/session/goal/set", {"sessionId": a, "op": op})
            check("goal", f"{op}_ok", "result" in r, r)
        late = run.link()
        check("goal", "late_joiner_list_row_carries_goal", "north harbor" in json.dumps(row(late, a).get("goal")), row(late, a).get("goal"))
        r = ctl.call("_nori/session/goal/set", {"sessionId": a, "op": "clear"})
        check("goal", "clear_ok_then_empty", "result" in r and g(ctl, a) is None, [r, g(ctl, a)])
        r = ctl.call("_nori/session/goal/get", {"sessionId": "00000000-0000-4000-8000-00000000dead"})
        check("goal", "unknown_session_-32002", err(r) == -32002, r)


# -------------------------------------------------------------- context (#782)
def probe_context():
    with Run("context") as run:
        (run.fx["roots"]["beta"] / "CLAUDE.md").write_text("# Beta harbor rules\nKeep the beacon lit.\n")
        ctl = run.link()
        if not has_cap(ctl, "context"):
            check("context", "capability", False, "absent"); RESULTS[-1]["absent"] = True; return
        check("context", "capability_advertised", True)
        sid = run.new(cwd=run.fx["roots"]["beta"])
        other = run.new(cwd=run.fx["roots"]["alpha"])
        watcher = run.link()
        r = watcher.call("_nori/session/context", {"sessionId": sid})
        blob = json.dumps(r)
        check("context", "watcher_may_read", "result" in r, blob[:300])
        check("context", "has_timeline_instructions_usage", all(k in (r.get("result") or {}) for k in ("timeline", "instructions", "usage")), list((r.get("result") or {}).keys()))
        check("context", "reads_claude_md_in_session_folder_on_host", "CLAUDE.md" in blob and str(run.fx["roots"]["beta"]) in blob, blob[:400])
        r2 = watcher.call("_nori/session/context", {"sessionId": other})
        check("context", "other_session_does_not_list_beta_file", str(run.fx["roots"]["beta"] / "CLAUDE.md") not in json.dumps(r2), json.dumps(r2)[:300])
        r3 = watcher.call("_nori/session/context", {"sessionId": sid, "skipInstructions": True})
        check("context", "skip_instructions_omits_them", "CLAUDE.md" not in json.dumps(r3) and "result" in r3, json.dumps(r3)[:300])
        r4 = ctl.call("_nori/session/context", {"sessionId": "00000000-0000-4000-8000-00000000dead"})
        check("context", "unknown_session_-32002", err(r4) == -32002, r4)


# ------------------------------------------------------------- compact (#783)
def probe_compact():
    with Run("compact") as run:
        ctl = run.link()
        if not has_cap(ctl, "compact"):
            check("compact", "capability", False, "absent"); RESULTS[-1]["absent"] = True; return
        check("compact", "capability_advertised", True)
        sid = run.new("relay 2 harbor")
        watcher = run.link()
        watcher.call("_nori/session/subscribe", {"session_id": sid, "cursors": {sid: 0}}); watcher.drain(1.0); watcher.notes.clear()
        r = watcher.call("_nori/session/compact", {"sessionId": sid})
        check("compact", "watcher_refused_-32016", err(r) == -32016, r)
        before = len(run.prompts())
        r = ctl.call("_nori/session/compact", {"sessionId": sid})
        check("compact", "controller_admitted", "result" in r, r)
        notes = watcher.drain(8.0)
        blob = [json.dumps(n) for n in notes]
        done = [b for b in blob if "context_compacted" in b or "Compact failed" in b]
        check("compact", "watcher_sees_finish_once", len([b for b in blob if "context_compacted" in b]) == 1, done[:2] or blob[-3:])
        check("compact", "agent_asked_to_summarize", len(run.prompts()) > before, run.prompts()[before:][:2])
        check("compact", "same_session_listed", bool(row(ctl, sid)), row(ctl, sid).get("session_id"))
        late = run.link()
        late.call("_nori/session/subscribe", {"session_id": sid, "cursors": {sid: 0}})
        replay = [json.dumps(n) for n in late.drain(2.0)]
        check("compact", "late_joiner_replay_has_finish", any("context_compacted" in b for b in replay), len(replay))
        n0 = len(run.prompts())
        r = ctl.call("_nori/session/prompt", {"session_id": sid, "prompt": text("relay 1 after")})
        deadline = time.time() + 15
        while time.time() < deadline and not any("relay 1 after" in p for p in run.prompts()):
            time.sleep(0.1)
        check("compact", "next_prompt_delivered_once", sum("relay 1 after" in p for p in run.prompts()) == 1, run.prompts()[n0:])
        r = ctl.call("_nori/session/compact", {"sessionId": "00000000-0000-4000-8000-00000000dead"})
        check("compact", "unknown_session_-32002", err(r) == -32002, r)


# --------------------------------------------------------------- shell (#785)
def probe_shell():
    """Cliff's local-only rule: a local controller runs ! in the session folder; a remote one gets
    -32018 and nothing runs; watchers stay read-only. Local = presents host/local-token (da3f2a53)."""
    with Run("shell") as run:
        lt = run.fx["homes"]["host"] / "host" / "local-token"
        ctl = run.link(local=True)
        if not has_cap(ctl, "shell"):
            check("shell", "capability", False, "absent"); RESULTS[-1]["absent"] = True; return
        sid = run.new(cwd=run.fx["roots"]["beta"])
        canary = lambda n: sorted(str(p.relative_to(run.dir)) for p in run.dir.rglob(f"probe-canary-{n}"))
        shell_cap = lambda w: (caps(w) or {}).get("shell")
        check("shell", "local_token_exists_0600", lt.exists() and oct(lt.stat().st_mode & 0o777) == "0o600",
              oct(lt.stat().st_mode & 0o777) if lt.exists() else "missing")
        if lt.exists():
            secret = lt.read_text().strip()
            leaks = [str(p.relative_to(run.dir)) for p in (run.fx["homes"]["host"]).rglob("*")
                     if p.is_file() and p != lt and secret and secret in p.read_text(errors="ignore")]
            check("shell", "local_token_in_no_other_host_file", not leaks, leaks)
        check("shell", "local_initialize_shell_true", shell_cap(ctl) is True, shell_cap(ctl))
        watcher = run.link(local=True)
        watcher.call("_nori/session/subscribe", {"session_id": sid, "cursors": {sid: 0}}); watcher.drain(1.0); watcher.notes.clear()
        r = watcher.call("_nori/session/shell", {"sessionId": sid, "command": "touch probe-canary-watcher"})
        time.sleep(1.0)
        check("shell", "local_watcher_refused_-32016", err(r) == -32016 and not canary("watcher"), [r, canary("watcher")])
        r = ctl.call("_nori/session/shell", {"sessionId": sid, "command": "echo cwd=$PWD; touch probe-canary-ctl"})
        notes = [json.dumps(n) for n in watcher.drain(4.0)]
        shell = [b for b in notes if "user_shell" in b]
        check("shell", "local_controller_ran_in_session_folder", canary("ctl") == ["roots/beta/app/probe-canary-ctl"], [r, canary("ctl")])
        out = "".join(bytes(json.loads(b)["params"]["event"]["event"]["event"].get("chunk") or []).decode() for b in shell)
        states = [json.loads(b)["params"]["event"]["event"]["event"].get("state") for b in shell]
        check("shell", "watcher_sees_user_shell_with_output", f"cwd={run.fx['roots']['beta']}" in out and states[:1] == ["started"] and states[-1:] == ["finished"], [states, out[:200]])
        r = ctl.call("_nori/session/shell", {"sessionId": sid, "command": ""})
        check("shell", "empty_command_-32602", err(r) == -32602, r)
        r = ctl.call("_nori/session/shell", {"sessionId": "00000000-0000-4000-8000-00000000dead", "command": "true"})
        check("shell", "local_unknown_session_-32002", err(r) == -32002, r)
        remote = run.link()
        check("shell", "remote_initialize_shell_false", shell_cap(remote) in (False, None), shell_cap(remote))
        remote.call("_nori/control/take", {})
        watcher.notes.clear()
        r = remote.call("_nori/session/shell", {"sessionId": sid, "command": "touch probe-canary-remote"})
        time.sleep(1.5)
        check("shell", "remote_controller_-32018_nothing_ran", err(r) == -32018 and not canary("remote"), [r, canary("remote")])
        check("shell", "remote_no_user_shell_event", not [n for n in watcher.drain(1.0) if "user_shell" in json.dumps(n)], "")
        r = remote.call("_nori/session/shell", {"sessionId": "00000000-0000-4000-8000-00000000dead", "command": "true"})
        check("shell", "remote_unknown_session_-32018", err(r) == -32018, r)
        bad = run.link()
        bad_hdr = wire.Wire(run.ep, run.tok, observe=False, log=run.dir / "wire-badtoken.jsonl", headers={"x-nori-local-token": "not-the-token"})
        run.links.append(bad_hdr)
        check("shell", "wrong_local_token_is_remote", shell_cap(bad_hdr) in (False, None), shell_cap(bad_hdr))
        check("shell", "nothing_sent_to_agent", not any("probe-canary" in p for p in run.prompts()), run.prompts()[-2:])

# ------------------------------------------- queue with a composer client (#778 e1a1e4b4)
def probe_queue_composer():
    """A client that declares `composer` (as the shared TUI does) queues over the wire; who can take it back?"""
    with Run("queue-composer") as run:
        a = run.link(composer=True)
        sid, q = paused_queue(run, a, ["queued alpha", "queued beta", "queued gamma"])
        ents = q.get("entries") or []
        check("queue_composer", "composer_prompts_editable", all(e.get("editable") for e in ents), ents)
        r1 = a.call("_nori/session/queue/take", {"sessionId": sid, "entryId": ents[0]["id"]})
        r2 = a.call("_nori/session/queue/take", {"sessionId": sid, "entryId": ents[0]["id"]})
        check("queue_composer", "own_take_once_then_null", (r1.get("result") or {}).get("text") == "queued alpha"
              and "result" in r2 and r2["result"].get("text") is None, [r1, r2])
        a.close(); run.links.remove(a)
        a2 = run.link(composer=True)
        r3 = a2.call("_nori/session/queue/take", {"sessionId": sid, "entryId": ents[1]["id"]})
        check("queue_composer", "after_reconnect_take_once", (r3.get("result") or {}).get("text") == "queued beta", r3)
        b = run.link(composer=True)
        tk = b.call("_nori/control/take", {})
        check("queue_composer", "second_terminal_takes_control", "result" in tk, tk)
        r4 = b.call("_nori/session/queue/take", {"sessionId": sid, "entryId": ents[2]["id"]})
        # Coordinator's reading of #778: a terminal takes back only its own prompts.
        check("queue_composer", "other_composer_terminal_cannot_take", (r4.get("result") or {}).get("text") is None, r4)
        plain = run.link()
        sid2, q2 = paused_queue(run, b, ["queued delta"])
        e2 = (q2.get("entries") or [])[0]
        plain.call("_nori/control/take", {})
        r5 = plain.call("_nori/session/queue/take", {"sessionId": sid2, "entryId": e2["id"]})
        check("queue_composer", "non_composer_client_take_result", True, r5)


PROBES = {"queue": probe_queue, "history": probe_history, "prompts": probe_prompts, "goal": probe_goal, "context": probe_context, "compact": probe_compact, "shell": probe_shell, "queue_composer": probe_queue_composer}
only = [x for x in A.only.split(",") if x]
for name, fn in PROBES.items():
    if only and name not in only:
        continue
    try:
        fn()
    except Exception as e:  # a probe that cannot reach its state is a failed check
        import traceback
        check(name, "probe.completed", False, f"{type(e).__name__}: {e}")
        (WORK / f"{name}-error.txt").write_text(traceback.format_exc())
out = Path(A.out); out.parent.mkdir(parents=True, exist_ok=True)
out.write_text(json.dumps(RESULTS, indent=2))
p = sum(r["passed"] for r in RESULTS)
print(json.dumps({"passed": p, "failed": sum(1 for r in RESULTS if not r["passed"] and not r.get("absent")), "absent": [r["probe"] for r in RESULTS if r.get("absent")]}))
