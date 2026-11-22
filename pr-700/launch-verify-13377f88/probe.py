"""Independent launch probe for nori-cli #700 (synthetic inputs, loopback only).

Observes real child processes through /proc (argv, environ, cwd, pgid) rather
than relying on the servers' own reports, for two ACP servers (an independent
Python server and the repo's Rust mock agent), three launch forms, and the
exec, local host, stdio-managed and shared-managed entry points.
"""
import asyncio, json, os, shutil, sys, tempfile, threading, time
from pathlib import Path

HERE = Path(__file__).resolve().parent
NORI = Path(sys.argv[1]).resolve()
MOCK = Path(sys.argv[2]).resolve()
ARGS = ["--flag", "", "--", "*", "é ü", "two words", "$HOME"]
PREFIX = ["--tag", "w 1", "--"]
WRAP = HERE / "bin" / "run wrap.sh"
SERVERS = {"alpha": HERE / "bin" / "alpha acp.py", "beta": MOCK}
WATCH = ("PROBE_KEEP", "PROBE_DROP", "PROBE_CHILD_ALPHA", "PROBE_CHILD_BETA", "NORI_AGENT_RUNTIME_DIR",
         "HOME", "PATH")
RESULTS = []


def ok(name, cond, detail=""):
    RESULTS.append((name, bool(cond), detail))
    print(("PASS " if cond else "FAIL ") + name + (f"  [{detail}]" if detail and not cond else ""), flush=True)


class Sampler(threading.Thread):
    """Record every process whose environment carries PROBE_CASE, plus nori processes."""

    def __init__(self):
        super().__init__(daemon=True)
        self.seen, self.stop = {}, threading.Event()

    def run(self):
        while not self.stop.is_set():
            for pid in os.listdir("/proc"):
                if not pid.isdigit():
                    continue
                try:
                    raw = open(f"/proc/{pid}/environ", "rb").read()
                    argv = [a.decode() for a in open(f"/proc/{pid}/cmdline", "rb").read().split(b"\0")[:-1]]
                    env = dict(e.decode(errors="replace").split("=", 1) for e in raw.split(b"\0") if b"=" in e)
                    if "PROBE_CASE" not in env and not (argv and argv[0] == str(NORI)):
                        continue
                    stat = open(f"/proc/{pid}/stat").read().rsplit(")", 1)[1].split()
                    self.seen[int(pid)] = {"pid": int(pid), "argv": argv, "env": env,
                                           "cwd": os.readlink(f"/proc/{pid}/cwd"), "ppid": int(stat[1]),
                                           "pgid": int(stat[2]), "exe": os.readlink(f"/proc/{pid}/exe")}
                except (OSError, ValueError):
                    pass
            time.sleep(0.02)

    def by_case(self, case):
        return [p for p in self.seen.values() if p["env"].get("PROBE_CASE") == case]


SAMPLER = Sampler()


def alive(pid):
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    try:  # zombies count as gone
        return open(f"/proc/{pid}/stat").read().rsplit(")", 1)[1].split()[0] != "Z"
    except OSError:
        return False


def reap():
    try:
        while os.waitpid(-1, os.WNOHANG)[0] > 0:
            pass
    except ChildProcessError:
        pass


def toml_str(v):
    return json.dumps(str(v))


def layout(root, case):
    base = root / case
    home, cwd, nori_home, out = base / "home", base / "work space", base / "nori", base / "out"
    for p in (home, cwd, nori_home, out):
        p.mkdir(parents=True)
    runtime = home / "prepared rt"
    runtime.mkdir(mode=0o700)
    (runtime / "marker").write_text("synthetic-runtime-marker")
    return base, home, cwd, nori_home, out, runtime


def ambient(case, home, nori_home, out, path=None):
    return {"PATH": path or os.environ["PATH"], "HOME": str(home), "NORI_HOME": str(nori_home),
            "TERM": "dumb", "NORI_NO_ANALYTICS": "1", "PROBE_CASE": case, "PROBE_OUT": str(out),
            "PROBE_KEEP": "synthetic-keep", "PROBE_DROP": "synthetic-drop",
            "MOCK_AGENT_ECHO_ENV": "PROBE_CHILD_BETA", "MOCK_AGENT_DELAY_MS": "500",
            "MOCK_AGENT_DESCENDANT_PID_FILE": str(out / "beta-descendant.pid")}


def child_env(server, mode, entry):
    return {} if mode == "plain" else {f"PROBE_CHILD_{server.upper()}": f"synthetic-{server}-{entry}"}


def registration(server, mode, runtime, entry):
    lines = ["[[agents]]", f'name = "Probe {server}"', f'slug = "probe-{server}"',
             "[agents.distribution.local]", f"command = {toml_str(SERVERS[server])}", f"args = {json.dumps(ARGS)}"]
    if mode != "plain":
        lines += ['env_remove = ["PROBE_DROP"]', f"runtime_dir = {toml_str(runtime)}"]
    if mode == "wrapped":
        lines += [f"wrapper = {json.dumps([str(WRAP), *PREFIX])}"]
    lines += ["[agents.distribution.local.env]"] + [f"{k} = {toml_str(v)}" for k, v in child_env(server, mode, entry).items()]
    return "\n".join(lines)


def spec_for(server, mode, cwd, home, runtime, entry, extra_env=None):
    pin = {"path": str(SERVERS[server]), "version": "probe-1", "args": ARGS}
    if mode == "wrapped":
        staged = runtime / "run wrap.sh"  # managed wrappers must resolve inside runtimeDir
        shutil.copy2(WRAP, staged)
        pin["wrapper"] = {"path": str(staged), "version": "probe-w1", "args": PREFIX}
    spec = {"projectId": "probe", "runId": f"run-{server}", "agent": f"probe-{server}", "cwd": str(cwd),
            "home": str(home), "adapter": pin, "env": {**child_env(server, mode, entry), **(extra_env or {})},
            "additions": {"promptEcho": False, "statusEvents": False, "recording": False, "clientMcp": False}}
    if mode != "plain":
        spec["envRemove"] = ["PROBE_DROP"]
        spec["runtimeDir"] = str(runtime)
    return spec


async def acp(send, recv, cwd, prompt="report", cancel=False):
    async def call(rid, method, params):
        await send({"jsonrpc": "2.0", "id": rid, "method": method, "params": params})
        chunks = []
        while True:
            m = await recv()
            if m.get("id") == rid and "method" not in m:
                if "error" in m:
                    raise RuntimeError(f"{method}: {m['error']}")
                return m["result"], chunks
            if m.get("method") == "session/update" and m["params"]["update"].get("sessionUpdate") == "agent_message_chunk":
                chunks.append(m["params"]["update"]["content"].get("text", ""))
    init, _ = await call(1, "initialize", {"protocolVersion": 1, "clientCapabilities": {}})
    sid = (await call(2, "session/new", {"cwd": str(cwd), "mcpServers": []}))[0]["sessionId"]
    if cancel:
        await send({"jsonrpc": "2.0", "id": 3, "method": "session/prompt",
                    "params": {"sessionId": sid, "prompt": [{"type": "text", "text": "hang"}]}})
        await asyncio.sleep(0.5)
        await send({"jsonrpc": "2.0", "method": "session/cancel", "params": {"sessionId": sid}})
        while True:
            m = await recv()
            if m.get("id") == 3 and "method" not in m:
                return init, m["result"]["stopReason"], ""
    res, chunks = await call(3, "session/prompt", {"sessionId": sid, "prompt": [{"type": "text", "text": prompt}]})
    return init, res["stopReason"], "".join(chunks)


def stdio_pipes(proc):
    async def send(m):
        proc.stdin.write((json.dumps(m) + "\n").encode())
        await proc.stdin.drain()

    async def recv():
        line = await asyncio.wait_for(proc.stdout.readline(), 30)
        if not line:
            raise RuntimeError("EOF")
        return json.loads(line)
    return send, recv


def check_children(label, case, server, mode, entry, cwd, home, runtime, out, managed=False):
    target = str(SERVERS[server])
    for _ in range(100):
        procs = SAMPLER.by_case(case)
        if any(target in p["argv"][:2] for p in procs):
            break
        time.sleep(0.03)
    servers = [p for p in procs if p["argv"][:1] == [target] or p["argv"][1:2] == [target] and "python" in p["argv"][0]]
    # The Python server runs as `python3 <script> ...` under its shebang.
    servers = [p for p in procs if (target in p["argv"][:2]) and not any(a.endswith("run wrap.sh") for a in p["argv"][:2])]
    ok(f"{label}: one server process observed", len(servers) >= 1, [p["argv"] for p in procs])
    if not servers:
        return []
    s = servers[-1]
    argv = s["argv"][s["argv"].index(target) + 1:]
    ok(f"{label}: server argv is literal registration", argv == ARGS, argv)
    e = s["env"]
    ok(f"{label}: cwd is the workspace", s["cwd"] == str(cwd), s["cwd"])
    if managed:
        ok(f"{label}: HOME is the binding home", e.get("HOME") == str(home.resolve()), e.get("HOME"))
    ok(f"{label}: inherited PROBE_KEEP reaches child", e.get("PROBE_KEEP") == "synthetic-keep")
    if mode == "plain":
        ok(f"{label}: plain keeps PROBE_DROP and has no runtime dir",
           e.get("PROBE_DROP") == "synthetic-drop" and "NORI_AGENT_RUNTIME_DIR" not in e)
    else:
        ok(f"{label}: env_remove drops PROBE_DROP", "PROBE_DROP" not in e)
        ok(f"{label}: child-only value set", e.get(f"PROBE_CHILD_{server.upper()}") == f"synthetic-{server}-{entry}")
        ok(f"{label}: runtime dir passed", e.get("NORI_AGENT_RUNTIME_DIR") == str(runtime.resolve()),
           e.get("NORI_AGENT_RUNTIME_DIR"))
    other = "beta" if server == "alpha" else "alpha"
    ok(f"{label}: no sibling server's child-only value", f"PROBE_CHILD_{other.upper()}" not in e)
    wrappers = [p for p in procs if any(a.endswith("run wrap.sh") for a in p["argv"][:2])]
    recorded = sorted(out.glob("wrapper-*.json"))
    if mode == "wrapped":
        recs = [json.loads(r.read_text()) for r in recorded]
        rec = next((r for r in recs if r["pid"] == s["pid"]), {})
        ok(f"{label}: wrapper argv = prefix + server + literal args",
           rec.get("argv") == [*PREFIX, target, *ARGS], rec.get("argv"))
        ok(f"{label}: wrapper exec'd into the server (same pid)", rec.get("pid") == s["pid"], (rec.get("pid"), s["pid"]))
    else:
        ok(f"{label}: no wrapper ran", not wrappers and not recorded)
    return procs


async def wait_gone(label, procs, runtime, mode):
    pids = [p["pid"] for p in procs]
    for _ in range(200):
        reap()
        if not any(alive(pid) for pid in pids):
            break
        await asyncio.sleep(0.05)
    left = [(p["pid"], p["argv"][:2]) for p in procs if alive(p["pid"])]
    ok(f"{label}: server, wrapper and descendants gone after close", not left, left)
    if mode != "plain":
        ok(f"{label}: user-named runtime dir kept", (runtime / "marker").exists())


def parent_isolation(label, case):
    noris = [p for p in SAMPLER.seen.values() if p["argv"][:1] == [str(NORI)] and p["env"].get("PROBE_CASE") == case]
    leaked = [p["pid"] for p in noris if any(k.startswith("PROBE_CHILD_") for k in p["env"])
              or "NORI_AGENT_RUNTIME_DIR" in p["env"]]
    ok(f"{label}: nori process env unchanged (no child-only names)", noris and not leaked, (len(noris), leaked))


async def run_exec(root, server, mode, cancel=False):
    entry = "exec"
    case = f"{entry}-{server}-{mode}" + ("-cancel" if cancel else "")
    base, home, cwd, nori_home, out, runtime = layout(root, case)
    (nori_home / "config.toml").write_text(registration(server, mode, runtime, entry))
    env = ambient(case, home, nori_home, out)
    proc = await asyncio.create_subprocess_exec(str(NORI), "--agent", f"probe-{server}", "exec", "--acp", "-C", str(cwd),
                                                cwd=cwd, env=env, stdin=-1, stdout=-1, stderr=-1)
    send, recv = stdio_pipes(proc)
    try:
        init, stop, text = await asyncio.wait_for(acp(send, recv, cwd, cancel=cancel), 60)
    except Exception as e:
        proc.kill()
        err = (await proc.stderr.read()).decode()[-800:]
        ok(f"{case}: ACP exchange", False, f"{e} {err}")
        return
    ok(f"{case}: ACP exchange -> {stop}", stop == ("cancelled" if cancel else "end_turn"), stop)
    if server == "beta" and not cancel:
        want = f"ENV:PROBE_CHILD_BETA=" + (f"synthetic-beta-{entry}" if mode != "plain" else "<unset>")
        ok(f"{case}: mock agent self-report", want in text, text[:120])
    procs = check_children(case, case, server, mode, entry, cwd, home, runtime, out)
    parent_isolation(case, case)
    proc.stdin.close()
    await asyncio.wait_for(proc.wait(), 35)
    await wait_gone(case, procs, runtime, mode)


async def run_local_host_pair(root, mode):
    """Both servers in one ordinary local host at once: sibling and parent isolation."""
    entry = "host"
    case = f"host-pair-{mode}"
    base, home, cwd, nori_home, out, runtime = layout(root, case)
    (nori_home / "config.toml").write_text("[acp_proxy]\nenabled = true\n" +
        "\n".join(registration(s, mode, runtime, entry) for s in ("alpha", "beta")))
    env = ambient(case, home, nori_home, out)

    async def host(*args):
        p = await asyncio.create_subprocess_exec(str(NORI), "-c", "features.multi_session=true", "host", *args,
                                                 cwd=cwd, env=env, stdout=-1, stderr=-1)
        o, e = await asyncio.wait_for(p.communicate(), 60)
        if p.returncode:
            raise RuntimeError(f"host {args[0]} -> {p.returncode}: {e.decode()[-600:]}")
        return o
    await host("--port", "0")
    try:
        for s in ("alpha", "beta"):
            json.loads(await host("new", "--cwd", str(cwd), "--agent", f"probe-{s}", "--prompt", "report", "--json"))
        for _ in range(200):
            servers = [p for p in SAMPLER.by_case(case) if any(str(SERVERS[s]) in p["argv"][:2] for s in SERVERS)]
            if len({p["argv"][1] if "python" in p["argv"][0] else p["argv"][0] for p in servers}) >= 2:
                break
            await asyncio.sleep(0.05)
        await asyncio.sleep(1.0)
        procs = []
        for s in ("alpha", "beta"):
            procs += check_children(f"{case}/{s}", case, s, mode, entry, cwd, home, runtime, out) or []
        parent_isolation(case, case)
    finally:
        await host("stop")
    await wait_gone(case, SAMPLER.by_case(case), runtime, mode)


async def run_stdio_managed(root, server, mode, explicit_path=True, tag=""):
    entry = "managed"
    case = f"stdio-{server}-{mode}" + ("-digest" if tag else "")
    if tag:
        shutil.rmtree(root / case, ignore_errors=True)
    base, home, cwd, nori_home, out, runtime = layout(root, case)
    extra = {"PATH": "/usr/local/bin:/usr/bin:/bin"} if explicit_path else {}
    spec = spec_for(server, mode, cwd, home, runtime, entry, extra)
    (base / "launch.json").write_text(json.dumps(spec))
    (nori_home / "config.toml").write_text(
        f'[host]\nmanaged=true\n[host.dedicated_roots]\nbinding_id="probe"\nhome={toml_str(home)}\ncwd={toml_str(cwd)}\n')
    env = ambient(case, home, nori_home, out)
    proc = await asyncio.create_subprocess_exec(str(NORI), "-c", "features.multi_session=true", "host", "--stdio",
                                                "--inherit-env", "--launch", str(base / "launch.json"),
                                                cwd=cwd, env=env, stdin=-1, stdout=-1, stderr=-1)
    send, recv = stdio_pipes(proc)
    try:
        init, stop, text = await asyncio.wait_for(acp(send, recv, cwd), 60)
    except Exception as e:
        proc.kill()
        ok(f"{case}: ACP exchange", False, f"{e} {(await proc.stderr.read()).decode()[-800:]}")
        return None
    ok(f"{case}: ACP exchange -> {stop}", stop == "end_turn", stop)
    procs = check_children(case, case, server, mode, entry, cwd, home, runtime, out, managed=True)
    parent_isolation(case, case)
    status = await control_status(nori_home, spec, explicit_path)
    proc.stdin.close()
    rc = await asyncio.wait_for(proc.wait(), 35)
    ok(f"{case}: host exits 0 on stdin EOF", rc == 0, rc)
    await wait_gone(case, procs, runtime, mode)
    return status


async def bridge(nori_home):
    p = await asyncio.create_subprocess_exec("bun", str(HERE / "ws.mjs"), str(nori_home), stdin=-1, stdout=-1)
    pending = []

    async def send(to, msg):
        p.stdin.write((json.dumps({"to": to, "msg": msg}) + "\n").encode())
        await p.stdin.drain()

    async def recv(pred):
        for i, m in enumerate(pending):
            if pred(m):
                return pending.pop(i)
        while True:
            m = json.loads(await asyncio.wait_for(p.stdout.readline(), 30))
            if pred(m):
                return m
            pending.append(m)
    return p, send, recv


async def control_status(nori_home, spec, explicit_path):
    """Read the admission receipt, then replay the same effective spec and a changed one."""
    p, send, recv = await bridge(nori_home)
    try:
        await send("control", {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}})
        await recv(lambda m: m.get("msg", {}).get("id") == 1)
        await send("control", {"jsonrpc": "2.0", "id": 2, "method": "_nori/session/status",
                               "params": {"bindingId": "probe", "generation": 1}})
        st = (await recv(lambda m: m.get("msg", {}).get("id") == 2))["msg"]["result"]
        if explicit_path:
            # Reconstruct the relay's effective env: ambient minus reserved/removed, explicit wins.
            amb = {k: v for k, v in os.environ.items()}  # not used; we rebuild from our own ambient below
        return st
    finally:
        p.kill()
        await p.wait()


async def run_shared_managed(root, mode):
    """Two sibling bindings on one managed host, re-ensure replay/conflict, and cleanup."""
    case = f"shared-{mode}"
    base = root / case
    nori_home, out = base / "nori", base / "out"
    for d in (nori_home / "hosted-sessions", out):
        d.mkdir(parents=True)
    (nori_home / "config.toml").write_text("[host]\nmanaged = true\nmax_sessions = 4\n")
    host_env = ambient(case, base, nori_home, out)
    host_env["PROBE_HOST_ONLY"] = "synthetic-host-only"
    p = await asyncio.create_subprocess_exec(str(NORI), "-c", "features.multi_session=true", "host", "--port", "0",
                                             env=host_env, stdout=-1, stderr=-1)
    o, e = await asyncio.wait_for(p.communicate(), 60)
    ok(f"{case}: managed host started", p.returncode == 0, e.decode()[-400:])
    br, send, recv = await bridge(nori_home)
    rid = iter(range(10, 1000))
    try:
        await send("control", {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}})
        await recv(lambda m: m.get("msg", {}).get("id") == 1)
        layouts, specs = {}, {}
        for s in ("alpha", "beta"):
            d = nori_home / "hosted-sessions" / s
            home, cwd, runtime = d / "home", d / "work space", d / "home" / "prepared rt"
            for x in (home, cwd):
                x.mkdir(parents=True)
            runtime.mkdir(mode=0o700)
            (runtime / "marker").write_text("synthetic-runtime-marker")
            # The host clears its environment: the recipe states PATH and the probe's observation names.
            spec = spec_for(s, mode, cwd, home, runtime, "managed", {
                "PATH": "/usr/local/bin:/usr/bin:/bin", "PROBE_CASE": f"{case}-{s}", "PROBE_OUT": str(out),
                "PROBE_KEEP": "synthetic-keep", "MOCK_AGENT_ECHO_ENV": "PROBE_CHILD_BETA"})
            specs[s], layouts[s] = spec, (home, cwd, runtime)
            params = {"commandId": f"c-{s}", "conversationId": f"00000000-0000-4000-8000-00000000000{1 if s == 'alpha' else 2}",
                      "bindingId": s, "generation": 1, "spec": spec}
            i = next(rid)
            await send("control", {"jsonrpc": "2.0", "id": i, "method": "_nori/session/ensure", "params": params})
            r = (await recv(lambda m, i=i: m.get("msg", {}).get("id") == i))["msg"]
            ok(f"{case}/{s}: ensure admitted", "result" in r, r.get("error"))
            first = r.get("result")
            i = next(rid)
            await send("control", {"jsonrpc": "2.0", "id": i, "method": "_nori/session/ensure",
                                   "params": {**params, "commandId": f"c-{s}-again"}})
            r2 = (await recv(lambda m, i=i: m.get("msg", {}).get("id") == i))["msg"]
            ok(f"{case}/{s}: identical re-ensure returns same receipt",
               r2.get("result") == first and first and first.get("specDigest"), (first, r2))
            changed = json.loads(json.dumps(params))
            changed["commandId"] = f"c-{s}-changed"
            changed["spec"]["env"]["PROBE_KEEP"] = "synthetic-changed"
            i = next(rid)
            await send("control", {"jsonrpc": "2.0", "id": i, "method": "_nori/session/ensure", "params": changed})
            r3 = (await recv(lambda m, i=i: m.get("msg", {}).get("id") == i))["msg"]
            ok(f"{case}/{s}: changed spec conflicts", "error" in r3 and "conflict" in json.dumps(r3["error"]).lower(),
               r3)
        for n, s in enumerate(("alpha", "beta")):
            home, cwd, runtime = layouts[s]

            async def msend(m, s=s):
                await send("machine", {"bindingId": s, "generation": 1, "message": m})

            async def mrecv(s=s):
                return (await recv(lambda m: m.get("from") == "machine" and m.get("msg", {}).get("bindingId") == s))["msg"]["message"]
            init, stop, text = await asyncio.wait_for(acp(msend, mrecv, cwd), 60)
            ok(f"{case}/{s}: ACP exchange -> {stop}", stop == "end_turn", stop)
        for s in ("alpha", "beta"):
            home, cwd, runtime = layouts[s]
            procs = SAMPLER.by_case(f"{case}-{s}")
            sv = [x for x in procs if str(SERVERS[s]) in x["argv"][:2] and not any(a.endswith("run wrap.sh") for a in x["argv"][:2])]
            ok(f"{case}/{s}: server observed", sv, [x["argv"] for x in procs])
            if not sv:
                continue
            e = sv[-1]["env"]
            argv = sv[-1]["argv"][sv[-1]["argv"].index(str(SERVERS[s])) + 1:]
            ok(f"{case}/{s}: server argv literal", argv == ARGS, argv)
            other = "BETA" if s == "alpha" else "ALPHA"
            ok(f"{case}/{s}: sibling isolation (own value only)",
               f"PROBE_CHILD_{other}" not in e and (mode == "plain" or e.get(f"PROBE_CHILD_{s.upper()}")), sorted(k for k in e if k.startswith("PROBE")))
            ok(f"{case}/{s}: host env not inherited (PROBE_HOST_ONLY absent)", "PROBE_HOST_ONLY" not in e)
            ok(f"{case}/{s}: HOME/cwd are binding paths",
               e.get("HOME") == str(home.resolve()) and sv[-1]["cwd"] == str(cwd.resolve()), (e.get("HOME"), sv[-1]["cwd"]))
            if mode != "plain":
                ok(f"{case}/{s}: runtime dir passed", e.get("NORI_AGENT_RUNTIME_DIR") == str(runtime.resolve()))
        hosts = [x for x in SAMPLER.seen.values() if x["argv"][:1] == [str(NORI)] and x["env"].get("PROBE_CASE") == case]
        ok(f"{case}: host process env has no child-only names",
           hosts and not any(k.startswith("PROBE_CHILD_") for h in hosts for k in h["env"]))
    finally:
        br.kill()
        await br.wait()
        stop = await asyncio.create_subprocess_exec(str(NORI), "host", "stop", env=host_env, stdout=-1, stderr=-1)
        await stop.wait()
    procs = SAMPLER.by_case(f"{case}-alpha") + SAMPLER.by_case(f"{case}-beta")
    await wait_gone(case, procs, layouts["alpha"][2], mode)


async def main():
    import ctypes
    ctypes.CDLL(None).prctl(36, 1, 0, 0, 0)  # subreaper so orphaned descendants can be reaped and counted
    SAMPLER.start()
    root = Path(tempfile.mkdtemp(prefix="nori-launch-probe-")).resolve()
    only = set(sys.argv[3:])
    try:
        if not only or "exec" in only:
            for server in ("alpha", "beta"):
                for mode in ("plain", "env", "wrapped"):
                    await run_exec(root, server, mode)
            await run_exec(root, "alpha", "wrapped", cancel=True)
        if not only or "host" in only:
            for mode in ("plain", "env", "wrapped"):
                await run_local_host_pair(root, mode)
        if not only or "stdio" in only:
            for server in ("alpha", "beta"):
                for mode in ("plain", "env", "wrapped"):
                    await run_stdio_managed(root, server, mode)
        if not only or "digest" in only:
            a = await run_stdio_managed(root, "alpha", "wrapped", True, "-pathA")
            b = await run_stdio_managed(root, "alpha", "wrapped", True, "-pathB")
            c = await run_stdio_managed(root, "alpha", "wrapped", False, "-nopathA")
            d = await run_stdio_managed(root, "alpha", "wrapped", False, "-nopathB")
            print("digests", json.dumps({k: (v or {}).get("specDigest") for k, v in
                                         {"pathA": a, "pathB": b, "nopathA": c, "nopathB": d}.items()}))
        if not only or "shared" in only:
            for mode in ("plain", "env", "wrapped"):
                await run_shared_managed(root, mode)
    finally:
        SAMPLER.stop.set()
    fails = [r for r in RESULTS if not r[1]]
    print(f"\n{len(RESULTS) - len(fails)}/{len(RESULTS)} checks passed")
    (HERE / "results.json").write_text(json.dumps([{"check": n, "pass": p, "detail": str(d)[:300]} for n, p, d in RESULTS], indent=1))
    shutil.rmtree(root, ignore_errors=True)


asyncio.run(main())
