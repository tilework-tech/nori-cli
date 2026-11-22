"""Compare the installed pi-acp and Pi under local and fixture-managed Nori."""
import argparse
import asyncio
from hashlib import sha256
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

HERE = Path(__file__).resolve().parent
PINNED = {"pi-acp": "0.0.35", "@earendil-works/pi-coding-agent": "1.6.0"}
ARGS = []  # Upstream pi-acp starts its stdio server without arguments.
PREFIX = ["--record", "../staging/.accept/wrapper.json", "--"]
PROMPT = "Read notes.txt, count its lines in a terminal, and report both."
ANSWER = "Read token=orchid-42; ran synthetic terminal: 3."
ACTIVE = "synthetic-accept-value"
AMBIENT = "synthetic-ambient-key"
KEY = "synthetic-pi-endpoint-key"
NATIVE_KEY = "synthetic-native-pi-key"
MARKER = "synthetic runtime marker"


def digest(value):
    return sha256(value.encode()).hexdigest() if value is not None else None


def private_json(path, obj):
    path.write_text(json.dumps(obj))
    path.chmod(0o600)


def records(path):
    return [json.loads(line) for line in path.read_text().splitlines()]


def prereqs(bindir):
    versions = {}
    for name, expected in PINNED.items():
        installed = json.loads((bindir.parent / name / "package.json").read_text())["version"]
        if installed != expected:
            raise ValueError(f"expected {name}@{expected}, found {installed}")
        versions[name] = installed
    return versions


def git(cwd, *args):
    return subprocess.check_output(
        ["git", "-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", *args],
        cwd=cwd, env={"PATH": os.environ["PATH"], "GIT_CONFIG_GLOBAL": os.devnull,
                      "GIT_CONFIG_NOSYSTEM": "1"},
    )


def prepare_dirty_worktree(cwd):
    repo = cwd.parent / "source"
    repo.mkdir()
    git(repo, "init", "--quiet", "--template=")
    (repo / "notes.txt").write_text("original synthetic note\n")
    git(repo, "add", "notes.txt")
    git(repo, "commit", "--quiet", "-m", "Add fixture note")
    git(repo, "worktree", "add", "--quiet", "--detach", str(cwd), "HEAD")
    (cwd / "draft.txt").write_text("pre-existing untracked draft\n")


def worktree_snapshot(cwd):
    return (git(cwd, "status", "--porcelain=v1", "--untracked-files=all", "-z"),
            git(cwd, "diff", "--binary", "HEAD"), (cwd / "draft.txt").read_bytes())


async def exchange(proc, cwd, dirty_snapshot=None):
    """Speak bare ACP to either the local facade or the host's managed bridge."""
    updates = []
    checked_during_turn = False

    async def send(frame):
        proc.stdin.write((json.dumps({"jsonrpc": "2.0", **frame}) + "\n").encode())
        await proc.stdin.drain()

    async def call(rid, method, params):
        nonlocal checked_during_turn
        await send({"id": rid, "method": method, "params": params})
        while True:
            line = await proc.stdout.readline()
            if not line:
                raise RuntimeError(f"unexpected EOF during {method}")
            frame = json.loads(line)
            if "method" not in frame and frame.get("id") == rid:
                if "error" in frame:
                    raise RuntimeError(f"{method} failed (code {frame['error']['code']})")
                return frame["result"]
            if frame.get("method") == "session/update":
                update = frame["params"]["update"]
                updates.append(update)
                if dirty_snapshot is not None and update.get("sessionUpdate") == "tool_call" and not checked_during_turn:
                    assert worktree_snapshot(cwd) == dirty_snapshot, "worktree changed during the ACP turn"
                    checked_during_turn = True
            elif frame.get("method") == "session/request_permission":
                options = frame["params"]["options"]
                allow = next(option for option in options if option["kind"] == "allow_once")
                await send({"id": frame["id"], "result": {"outcome": {
                    "outcome": "selected", "optionId": allow["optionId"]}}})

    init = await call(1, "initialize", {"protocolVersion": 1, "clientCapabilities": {}})
    assert init["protocolVersion"] == 1
    session = await call(2, "session/new", {"cwd": str(cwd), "mcpServers": []})
    turn = await call(3, "session/prompt", {"sessionId": session["sessionId"],
                      "prompt": [{"type": "text", "text": PROMPT}]})
    assert turn["stopReason"] == "end_turn", turn["stopReason"]
    text = "".join(update.get("content", {}).get("text", "") for update in updates
                   if update.get("sessionUpdate") == "agent_message_chunk")
    assert ANSWER in text, "missing read + terminal result"
    assert dirty_snapshot is None or checked_during_turn, "no live tool call observed"


async def local_host_turn(binary, cwd, config, env):
    """Check the underlying ACP exchange recorded by the normal local host."""
    async def host(*args):
        child = await asyncio.create_subprocess_exec(str(binary), "-c", "features.multi_session=true",
                    "host", *args, cwd=cwd, env=env, stdout=asyncio.subprocess.PIPE,
                    stderr=asyncio.subprocess.DEVNULL)
        output, _ = await asyncio.wait_for(child.communicate(), 40)
        assert child.returncode == 0, ("host", args[0], child.returncode)
        return output

    await host("--port", "0")
    try:
        started = json.loads(await host("new", "--cwd", str(cwd), "--agent", "fixture-pi",
                                        "--prompt", PROMPT, "--json"))
        assert started["session_id"]
        while True:
            await asyncio.sleep(0.1)
            files = list((config / "acp-wire").glob("*.jsonl"))
            if not files:
                continue
            # Only inspect complete newline-terminated records while the host is writing.
            raw = files[0].read_bytes().split(b"\n")[:-1]
            frames = [json.loads(line)["message"] for line in raw]
            completed = [frame for frame in frames if frame.get("result", {}).get("stopReason") == "end_turn"]
            if completed:
                methods = {frame.get("method"): frame.get("id") for frame in frames if frame.get("method")}
                for method in ["initialize", "session/new", "session/prompt"]:
                    assert any(frame.get("id") == methods[method] and "result" in frame for frame in frames)
                chunks = [frame["params"]["update"] for frame in frames if frame.get("method") == "session/update"]
                assert ANSWER in "".join(chunk["content"]["text"] for chunk in chunks
                                         if chunk["sessionUpdate"] == "agent_message_chunk")
                return
    finally:
        await host("stop")


def exited(identity, group=False):
    try:
        (os.killpg if group else os.kill)(identity, 0)
    except ProcessLookupError:
        return True
    return False


async def case(root, entry, mode, binary, bindir, base_url, endpoint_log):
    working = root / f"{entry}-{mode}"
    cwd, staging = working / "workspace", working / "staging"
    cwd.mkdir(parents=True, mode=0o700)
    verify_staging = entry == "managed" and mode == "wrapped"
    if verify_staging:
        prepare_dirty_worktree(cwd)
    (cwd / "notes.txt").write_text("token=orchid-42\nsecond synthetic line\nthird synthetic line\n")
    dirty_snapshot = worktree_snapshot(cwd) if verify_staging else None
    if verify_staging:
        assert dirty_snapshot[0] == b" M notes.txt\x00?? draft.txt\x00"
    home, nori_dir, evidence = staging / "home", staging / "nori", staging / ".accept"
    for path in [home, nori_dir, evidence]:
        path.mkdir(parents=True, mode=0o700)
    capture, wrap_capture = evidence / "children.jsonl", evidence / "wrapper.json"
    runtime = home / "prepared runtime" if mode != "plain" else None
    profile = runtime if runtime else home / ".pi" / "agent"
    profile.mkdir(mode=0o700, parents=True)
    if runtime:
        (runtime / "marker").write_text(MARKER)
    private_json(profile / "settings.json", {"defaultProvider": "lab", "defaultModel": "lab-small",
                                             "quietStartup": True})
    private_json(profile / "models.json", {"providers": {"lab": {
        "baseUrl": base_url, "api": "openai-completions",
        "apiKey": NATIVE_KEY if mode == "plain" else "$ACCEPT_LOOPBACK_KEY",
        "models": [{"id": "lab-small", "name": "Lab Small"}]}}})
    adapter, pi = bindir / "pi-acp", bindir / "pi"
    registered = staging / "registered pi"
    override = runtime / "selected pi" if mode == "inner" else None
    for shim, record in [(registered, "pi.json"), (override, "inner.json")]:
        if shim is None:
            continue
        shim.write_text(f"""#!/bin/sh
python3 -c 'import json,sys;json.dump(sys.argv[1:],open("../staging/.accept/{record}","w"))' "$@" || exit 68
exec "$FIXTURE_PI" "$@"
""")
        shim.chmod(0o700)
    base = {"PATH": os.pathsep.join([str(bindir), str(Path(shutil.which("node")).parent), os.defpath]),
            "TERM": "dumb", "NORI_NO_ANALYTICS": "1", "PI_SKIP_VERSION_CHECK": "1", "PI_TELEMETRY": "0",
            "NODE_OPTIONS": "--require=" + json.dumps(str(HERE / "observe.cjs")),
            "FIXTURE_CAPTURE": str(capture), "FIXTURE_ADAPTER": str(adapter.resolve()),
            "FIXTURE_PI": str(pi.resolve()),
            "ACCEPT_AMBIENT_KEY": AMBIENT}
    selected = {} if mode == "plain" else {"NORI_ACCEPT_VALUE": ACTIVE, "ACCEPT_LOOPBACK_KEY": KEY}
    if override:
        selected["PI_ACP_PI_COMMAND"] = str(override)
    if runtime:
        selected["PI_CODING_AGENT_DIR"] = str(runtime)
    launch_env = {"PI_ACP_PI_COMMAND": str(registered), **selected}
    if mode == "wrapped":
        wrapper = (runtime if entry == "managed" else staging) / "run wrapper"
        wrapper.write_text("""#!/bin/sh
[ "$1" = "--record" ] && [ "$2" = "../staging/.accept/wrapper.json" ] && [ "$3" = "--" ] || exit 67
python3 - "$@" <<'PYWRAPPER' || exit 66
import hashlib, json, os, sys
def fingerprint(key):
    val = os.environ.get(key)
    return hashlib.sha256(val.encode()).hexdigest() if val is not None else None
with open(sys.argv[2], "w") as stream:
    json.dump({"argv": sys.argv[1:], "variables": {key: fingerprint(key)
               for key in ["NORI_ACCEPT_VALUE", "ACCEPT_AMBIENT_KEY", "ACCEPT_LOOPBACK_KEY"]}}, stream)
PYWRAPPER
shift 3
exec "$@"
""")
        wrapper.chmod(0o700)
    env = {**base, "NORI_HOME": str(nori_dir), "HOME": str(home)}
    if entry != "managed":
        definition = ["[acp_proxy]", f"enabled = {'true' if entry == 'host' else 'false'}",
                      "[[agents]]", 'name = "Fixture Pi"', 'slug = "fixture-pi"',
                      "[agents.distribution.local]", f"command = {json.dumps(str(adapter))}",
                      f"args = {json.dumps(ARGS)}"]
        if selected:
            definition += ['env_remove = ["ACCEPT_AMBIENT_KEY"]']
        if runtime:
            definition += [f"runtime_dir = {json.dumps(str(runtime))}"]
        if mode == "wrapped":
            definition += [f"wrapper = {json.dumps([str(wrapper), *PREFIX])}"]
        definition += ["[agents.distribution.local.env]"]
        definition += [f"{k} = {json.dumps(v)}" for k, v in launch_env.items()]
        (nori_dir / "config.toml").write_text("\n".join(definition))
        args = ["--agent", "fixture-pi", "exec", "--acp", "-C", str(cwd)]
    else:
        pin = {"path": str(adapter), "version": PINNED["pi-acp"], "args": ARGS}
        spec = {"projectId": "lab", "runId": "synthetic", "agent": "pi", "cwd": str(cwd),
                "home": str(home), "adapter": pin, "env": launch_env,
                "envRemove": ["ACCEPT_AMBIENT_KEY"] if selected else [],
                "additions": {"promptEcho": False, "clientMcp": False,
                              "statusEvents": False, "recording": False}}
        if runtime:
            spec["runtimeDir"] = str(runtime)
        if mode == "wrapped":
            pin["wrapper"] = {"path": str(wrapper), "args": PREFIX, "version": "fixture-1"}
        private_json(staging / "launch.json", spec)
        (nori_dir / "config.toml").write_text(
            f'[host]\nmanaged=true\n[host.dedicated_roots]\nbinding_id="lab"\n'
            f'home={json.dumps(str(home))}\ncwd={json.dumps(str(cwd))}\n')
        args = ["-c", "features.multi_session=true", "host", "--stdio", "--inherit-env", "--launch", str(staging / "launch.json")]

    start = len(records(endpoint_log))
    if entry == "host":
        await asyncio.wait_for(local_host_turn(binary, cwd, nori_dir, env), 90)
    else:
        proc = await asyncio.create_subprocess_exec(str(binary), *args, cwd=cwd, env=env,
                stdin=asyncio.subprocess.PIPE, stdout=asyncio.subprocess.PIPE,
                stderr=asyncio.subprocess.DEVNULL, limit=2**20)
        try:
            await asyncio.wait_for(exchange(proc, cwd, dirty_snapshot), 90)
        finally:
            proc.stdin.close()
            try:
                await asyncio.wait_for(proc.wait(), 35)
            except asyncio.TimeoutError:
                proc.terminate()
                await proc.wait()
                raise
        assert proc.returncode == 0, (entry, mode, proc.returncode)
    rows = records(capture)
    servers = [row for row in rows if row["role"] == "adapter"]
    assert len(servers) == 1 and servers[0]["argv"] == ARGS, "registered argv changed"
    for _ in range(40):
        if sys.platform == "linux":
            # pi-acp can exit before Node has reaped Pi. Adopted children have
            # already stopped when waitpid returns their PID; do not signal them.
            try:
                while os.waitpid(-servers[0]["pid"], os.WNOHANG)[0] > 0:
                    pass
            except ChildProcessError:
                pass
        if exited(servers[0]["pid"], group=True) and all(exited(row["pid"]) for row in rows):
            break
        await asyncio.sleep(0.05)
    else:
        raise AssertionError("adapter process group or observed Pi child survived shutdown")
    rpc = [row["argv"] for row in rows if row["role"] == "pi" and row["argv"][:2] == ["--mode", "rpc"]]
    assert len(rpc) == 1 and "--" not in rpc[0], "unexpected Pi RPC command"
    inner_args = rpc[0]
    expected = {"ACCEPT_AMBIENT_KEY": digest(AMBIENT if mode == "plain" else None),
                "NORI_ACCEPT_VALUE": digest(selected.get("NORI_ACCEPT_VALUE")),
                "ACCEPT_LOOPBACK_KEY": digest(selected.get("ACCEPT_LOOPBACK_KEY"))}
    for row in rows:
        assert row["cwd"] == str(cwd) and row["home"] == str(home)
        assert row["variables"] == expected, row["variables"]
        assert row["runtime"] == (str(runtime) if runtime else None)
        assert row["mode"] == (0o700 if runtime else None)
        assert row["marker"] == digest(MARKER if runtime else None)
        assert row["inner"] == launch_env["PI_ACP_PI_COMMAND"]
    for record, should_exist in [("pi.json", mode != "inner"), ("inner.json", mode == "inner")]:
        path = evidence / record
        assert path.exists() == should_exist, f"unexpected inner executable for {mode}"
        if should_exist:
            assert json.loads(path.read_text()) == inner_args, "inner command changed Pi's argv"
    if mode == "wrapped":
        assert json.loads(wrap_capture.read_text()) == {
            "argv": [*PREFIX, str(adapter), *ARGS], "variables": expected}
    else:
        assert not wrap_capture.exists()
    chats = [row for row in records(endpoint_log)[start:] if row["path"].endswith("/chat/completions")]
    assert len(chats) == 3, len(chats)
    key = KEY if selected else NATIVE_KEY
    assert all(row["authorizationFingerprint"] == digest("Bearer " + key) and
               row["model"] == "lab-small" for row in chats)
    summary = {"argv": servers[0]["argv"], "variables": servers[0]["variables"],
               "runtimeMode": servers[0]["mode"], "marker": servers[0]["marker"],
               "innerOverride": mode == "inner", "innerArgs": inner_args,
               "wrapperArgs": [*PREFIX, "<adapter>", *ARGS] if mode == "wrapped" else None}
    for path in [endpoint_log, *working.rglob("*.jsonl"), *nori_dir.rglob("*.log")]:
        content = path.read_bytes()
        assert all(value.encode() not in content for value in (KEY, NATIVE_KEY, ACTIVE, AMBIENT)), \
            "unhashed synthetic credential or test value in a log"
    if verify_staging:
        shutil.rmtree(staging)
        assert worktree_snapshot(cwd) == dirty_snapshot, "worktree changed after runtime cleanup"
        print("managed wrapped: tracked edits and untracked files preserved before staging, during turn and after cleanup", flush=True)
    checksum = digest(json.dumps(summary, sort_keys=True))
    print(f"{entry} {mode}: end_turn, read + terminal, {len(chats)} calls; inputs SHA-256 {checksum}", flush=True)
    return checksum


async def verify(binary, bindir):
    if sys.platform == "linux":
        import ctypes
        if ctypes.CDLL(None, use_errno=True).prctl(36, 1, 0, 0, 0) != 0:  # PR_SET_CHILD_SUBREAPER
            raise OSError(ctypes.get_errno(), "cannot adopt exited fixture children")
    print("Installed servers", json.dumps(prereqs(bindir)), flush=True)
    with tempfile.TemporaryDirectory(prefix="nori-pi-matrix-") as temp:
        root = Path(temp).resolve()
        log = root / "model-requests.jsonl"
        log.touch()
        server = await asyncio.create_subprocess_exec("bun", str(HERE / "endpoint.mjs"), str(log),
            stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.DEVNULL,
            env={"PATH": os.environ["PATH"], "HOME": str(root)})
        try:
            url = (await asyncio.wait_for(server.stdout.readline(), 10)).decode().strip()
            assert url.startswith("http://127.0.0.1:")
            for mode in ["plain", "env", "wrapped", "inner"]:
                local = await case(root, "local", mode, binary, bindir, url, log)
                host = await case(root, "host", mode, binary, bindir, url, log)
                managed = await case(root, "managed", mode, binary, bindir, url, log)
                assert local == host == managed, "local and fixture-managed inputs differ"
        finally:
            server.terminate()
            await server.wait()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("nori", type=Path)
    parser.add_argument("bin_dir", type=Path, help="node_modules/.bin containing pinned pi-acp and pi")
    options = parser.parse_args()
    asyncio.run(verify(options.nori.resolve(), options.bin_dir.resolve()))
