"""Managed outer-wrapper confinement at 13377f88 through the stdio admission path (synthetic)."""
import json, os, shutil, subprocess, sys, tempfile
from pathlib import Path
NORI, SERVER, WRAP = Path(sys.argv[1]).resolve(), Path(sys.argv[2]).resolve(), Path(sys.argv[3]).resolve()
root = Path(tempfile.mkdtemp(prefix="nori-wrap-refusal-")).resolve()
rows = []
def case(name, prepare, expect_ok):
    base = root / name; home, cwd, nh = base / "home", base / "work", base / "nori"
    for d in (home, cwd, nh): d.mkdir(parents=True)
    rt = home / "rt"; rt.mkdir()
    outside = base / "outside"; outside.mkdir()
    spec = {"projectId": "p", "runId": "r", "agent": "a", "cwd": str(cwd), "home": str(home),
            "adapter": {"path": str(SERVER), "version": "1", "args": []},
            "env": {"PATH": "/usr/local/bin:/usr/bin:/bin", "PROBE_OUT": str(base), "PROBE_CASE": name},
            "additions": {"promptEcho": False, "statusEvents": False, "recording": False, "clientMcp": False}}
    prepare(spec, rt, outside)
    (base / "launch.json").write_text(json.dumps(spec))
    (nh / "config.toml").write_text(f'[host]\nmanaged=true\n[host.dedicated_roots]\nbinding_id="b"\nhome={json.dumps(str(home))}\ncwd={json.dumps(str(cwd))}\n')
    proc = subprocess.Popen([str(NORI), "-c", "features.multi_session=true", "host", "--stdio", "--launch", str(base / "launch.json")],
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
        env={"PATH": os.environ["PATH"], "HOME": str(home), "NORI_HOME": str(nh), "NORI_NO_ANALYTICS": "1"})
    proc.stdin.write('{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":1}}\n'); proc.stdin.flush()
    first = proc.stdout.readline()
    proc.stdin.close(); proc.stdin = None
    rest, err = proc.communicate(timeout=60)
    p = subprocess.CompletedProcess(proc.args, proc.returncode, first + rest, err)
    spawned = any(base.glob("wrapper-*.json")) or any(base.glob("alpha-*.json"))
    initialized = '"result"' in p.stdout and '"protocolVersion"' in p.stdout
    reason = [l for l in p.stderr.splitlines() if "wrapper" in l.lower() or "runtime" in l.lower()][-1:] or p.stderr.strip().splitlines()[-1:]
    good = (initialized and p.returncode == 0) if expect_ok else (p.returncode != 0 and not p.stdout.strip() and not spawned)
    rows.append({"case": name, "expected": "admitted" if expect_ok else "refused before spawn", "exit": p.returncode,
                 "stdoutEmpty": not p.stdout.strip(), "spawned": spawned, "pass": good,
                 "reason": (reason[0] if reason else "")[-160:]})
    print(("PASS " if good else "FAIL ") + json.dumps(rows[-1], ensure_ascii=False))
def staged(rt, name="run wrap.sh"):
    t = rt / name; shutil.copy2(WRAP, t); return t
W = lambda path: {"path": str(path), "version": "w1", "args": ["--tag", "w 1", "--"]}
def inside(s, rt, o): s["runtimeDir"] = str(rt); s["adapter"]["wrapper"] = W(staged(rt))
def outside(s, rt, o): s["runtimeDir"] = str(rt); s["adapter"]["wrapper"] = W(staged(o))
def no_runtime(s, rt, o): s["adapter"]["wrapper"] = W(staged(rt))
def symlink_out(s, rt, o):
    s["runtimeDir"] = str(rt); (rt / "link").symlink_to(staged(o)); s["adapter"]["wrapper"] = W(rt / "link")
def symlink_in(s, rt, o):
    s["runtimeDir"] = str(rt); (o / "link").symlink_to(staged(rt)); s["adapter"]["wrapper"] = W(o / "link")
def dotdot(s, rt, o):
    s["runtimeDir"] = str(rt); staged(o); s["adapter"]["wrapper"] = W(Path(str(rt) + "/../../outside/run wrap.sh"))
def is_dir(s, rt, o): s["runtimeDir"] = str(rt); s["adapter"]["wrapper"] = W(rt)
def system_path(s, rt, o): s["runtimeDir"] = str(rt); s["adapter"]["wrapper"] = W(Path("/bin/sh"))
def no_version(s, rt, o): inside(s, rt, o); s["adapter"]["wrapper"]["version"] = " "
case("inside-runtime", inside, True)
case("outside-runtime", outside, False)
case("missing-runtimeDir", no_runtime, False)
case("symlink-inside-to-outside", symlink_out, False)
case("symlink-outside-to-inside", symlink_in, True)
case("dotdot-escape", dotdot, False)
case("system-path-/bin/sh", system_path, False)
case("blank-version", no_version, False)
case("wrapper-is-runtimeDir", is_dir, False)
Path("refusals.json").write_text(json.dumps(rows, indent=1, ensure_ascii=False))
shutil.rmtree(root)
