"""End-to-end `@` trace on a joined `nori attach --host` with separate host and client homes.
Each step records the session the hint row names, every find_paths request the relay saw
(session -> folder), the host's root and paths, and the popup's first row."""
import json, os, re, shlex, subprocess, sys, time
from pathlib import Path
T = Path(open(Path(__file__).with_name("last")).read().strip()); B, label, relay_port = sys.argv[1], sys.argv[2], sys.argv[3]
relay = T / "relay.jsonl"
sock = f"l3p{os.getpid()}"
def tm(*a): return subprocess.run(["tmux", "-f", "/dev/null", "-L", sock, *a], capture_output=True, text=True).stdout
env = {"HOME": str(T / "clienthome"), "NORI_HOME": str(T / "clienthome"), "PATH": os.environ["PATH"], "TERM": "xterm-256color", "LANG": "C.UTF-8"}
cmd = shlex.join(["env", "-i", *[f"{k}={v}" for k, v in env.items()], B, "attach", "--host", f"ws://127.0.0.1:{relay_port}/acp", "--token-file", str(T / "hosthome/host/token")])
tm("new-session", "-d", "-s", "k", "-x", "110", "-y", "32", "-c", str(T / "clientdir"), cmd)
hostenv = {**env, "HOME": str(T / "hosthome"), "NORI_HOME": str(T / "hosthome")}
ls = json.loads(subprocess.run([B, "host", "ls", "--json"], env=hostenv, cwd=T / "hostdir", capture_output=True, text=True).stdout)
folders = {}
for line in subprocess.run([B, "host", "ls"], env=hostenv, cwd=T / "hostdir", capture_output=True, text=True).stdout.splitlines()[1:]:
    parts = line.split(); folders[parts[0]] = next(p for p in parts if p.startswith("/"))
def wire_since(n):
    rows = [json.loads(l) for l in relay.read_text().splitlines()[n:]]
    return [{"session": folders.get((r["for"].get("sessionId") or ""), "(none: host folder)"), "root": r["reply"].get("root"),
             "paths": [m["path"] for m in r["reply"].get("matches", [])]} for r in rows if "reply" in r]
def screen(): return [l for l in tm("capture-pane", "-p", "-t", "k").splitlines() if l.strip()]
def hint():
    for _ in range(50):
        row = next((l for l in reversed(screen()) if "controlling" in l or "watching" in l), "")
        m = re.search(r"controlling · ([^·]+) · kiln · ", row)
        if m: return m.group(1).strip()
        time.sleep(0.02)
    sys.stderr.write("HINT-MISS\n" + "\n".join(screen()[-5:]) + "\n"); return "(no session)"
steps = []; current = {"shown": "(no session)"}
def ask(name, q):
    n = len(relay.read_text().splitlines()); shown = current["shown"]
    tm("send-keys", "-t", "k", "-l", "@" + q); time.sleep(1.2)
    s = screen(); i = max(i for i, l in enumerate(s) if l.lstrip().startswith("›"))
    steps.append({"step": name, "query": q, "shown_session": shown, "wire": wire_since(n), "popup": s[i + 1].strip() if i + 1 < len(s) else ""})
    tm("send-keys", "-t", "k", "C-u"); time.sleep(0.3)
def pick(direction):
    tm("send-keys", "-t", "k", "M-s"); time.sleep(0.6); tm("send-keys", "-t", "k", direction); time.sleep(0.1); tm("send-keys", "-t", "k", "Enter")
    time.sleep(0.4); current["shown"] = hint()
time.sleep(3); tm("send-keys", "-t", "k", "Escape"); time.sleep(1)
for q in ["HostOnlyQ", "AmberOnlyL", "ClientOnlyF"]: ask("no session", q)
ask("no session, same text again", "HostOnlyQ")
for want, key in [("birch", "j"), ("amber", "k"), ("birch", "j")]:
    pick(key)
    for q in ["BirchOnlyW", "AmberOnlyL"]: ask(f"right after Alt+S to {want}", q)
    ask(f"{want}, same text again", "BirchOnlyW" if want == "birch" else "AmberOnlyL")
tm("kill-server")
json.dump({"build": label, "steps": steps}, sys.stdout); print()
