#!/usr/bin/env python3
"""Independent minimal ACP server used only for launch verification (synthetic)."""
import hashlib, json, os, subprocess, sys, threading
def fp(name):
    v = os.environ.get(name)
    return None if v is None else hashlib.sha256(v.encode()).hexdigest()[:12]
desc = subprocess.Popen(["sleep", "600"])  # descendant in the server's process group
out = os.environ.get("PROBE_OUT")
if out:
    with open(os.path.join(out, f"alpha-{os.getpid()}.json"), "w") as f:
        json.dump({"pid": os.getpid(), "descendant": desc.pid, "argv": sys.argv[1:], "cwd": os.getcwd(),
                   "runtime": os.environ.get("NORI_AGENT_RUNTIME_DIR")}, f)
cancel = threading.Event()
lock = threading.Lock()
def send(obj):
    with lock:
        sys.stdout.write(json.dumps(obj) + "\n"); sys.stdout.flush()
def prompt(rid, sid, text):
    if "hang" in text:
        cancel.wait(30)
        send({"jsonrpc": "2.0", "id": rid, "result": {"stopReason": "cancelled"}}); return
    seen = {"server": "alpha", "argv": sys.argv[1:], "runtime": os.environ.get("NORI_AGENT_RUNTIME_DIR"),
            "vars": {k: fp(k) for k in ["PROBE_KEEP", "PROBE_DROP", "PROBE_CHILD_ALPHA", "PROBE_CHILD_BETA"]}}
    send({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": sid, "update": {
        "sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "SEEN " + json.dumps(seen)}}}})
    send({"jsonrpc": "2.0", "id": rid, "result": {"stopReason": "end_turn"}})
for line in sys.stdin:
    if not line.strip(): continue
    m = json.loads(line); meth = m.get("method"); rid = m.get("id")
    if meth == "initialize":
        send({"jsonrpc": "2.0", "id": rid, "result": {"protocolVersion": 1, "agentCapabilities": {},
              "agentInfo": {"name": "alpha-probe", "version": "0.1.0"}}})
    elif meth == "session/new":
        send({"jsonrpc": "2.0", "id": rid, "result": {"sessionId": "alpha-1"}})
    elif meth == "session/prompt":
        text = "".join(b.get("text", "") for b in m["params"]["prompt"])
        threading.Thread(target=prompt, args=(rid, m["params"]["sessionId"], text), daemon=True).start()
    elif meth == "session/cancel":
        cancel.set()
    elif rid is not None:
        send({"jsonrpc": "2.0", "id": rid, "error": {"code": -32601, "message": "unsupported"}})
