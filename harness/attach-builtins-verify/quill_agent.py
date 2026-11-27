"""Quill: a synthetic ACP agent server for attach verification.

usage: quill_agent.py <display-name> [--modes a,b] [--bare]

  --modes  also advertise a native permission `mode` option with these values
  --bare   advertise no config options at all

Behaviour is driven by files in a control directory ($QUILL_CTL, default
the process cwd), so a test can change it while the host is running:
  delay-config    seconds to wait before answering session/set_config_option
  reject-model    a model value the agent refuses with a JSON-RPC error
Every answer to the prompt "status" names the agent, session, model, effort
and the agent process cwd, so the screen shows what the agent really uses.
"""
import itertools
import json
import os
import sys
import threading
import time
from pathlib import Path

NAME = sys.argv[1]
BARE = "--bare" in sys.argv
MODES = sys.argv[sys.argv.index("--modes") + 1].split(",") if "--modes" in sys.argv else []
CTL = Path(os.environ.get("QUILL_CTL", os.getcwd()))
MODELS = ["harbor-mini", "harbor-max", f"{NAME.lower()}-classic"]
EFFORTS = ["low", "medium", "high"]
counter = itertools.count(1)
state = {}
lock = threading.Lock()


def send(**fields):
    with lock:
        sys.stdout.write(json.dumps({"jsonrpc": "2.0", **fields}) + "\n")
        sys.stdout.flush()


def log(text):
    with open(CTL / f"quill-{NAME.lower()}.log", "a") as out:
        out.write(f"{time.strftime('%H:%M:%S')} {text}\n")


def config(sid):
    s = state[sid]
    if BARE:
        return []
    mode = [{"type": "select", "category": "mode", "id": "mode", "name": "Mode",
             "currentValue": s["mode"], "options": [{"value": m, "name": m} for m in MODES]}] if MODES else []
    return mode + [
        {"type": "select", "category": "model", "id": "model", "name": "Model",
         "currentValue": s["model"], "options": [{"value": m, "name": m} for m in MODELS]},
        {"type": "select", "category": "thought_level", "id": "effort", "name": "Effort",
         "currentValue": s["effort"], "options": [{"value": e, "name": e.title()} for e in EFFORTS]},
    ]


def say(sid, text):
    send(method="session/update", params={"sessionId": sid, "update": {
        "sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": text}}})


def set_option(rid, params):
    sid, key, value = params["sessionId"], params["configId"], params["value"]
    delay = CTL / "delay-config"
    if delay.exists():
        time.sleep(float(delay.read_text().strip() or "3"))
    reject = CTL / "reject-model"
    if key == "model" and reject.exists() and reject.read_text().strip() == value:
        log(f"reject {sid} model={value}")
        send(id=rid, error={"code": -32602, "message": f"{NAME} refuses model {value}"})
        return
    allowed = None if BARE else {"model": MODELS, "effort": EFFORTS, "mode": MODES or None}.get(key)
    if allowed is None or value not in allowed:
        send(id=rid, error={"code": -32602, "message": f"unknown option {key}={value}"})
        return
    state[sid][key] = value
    log(f"set {sid} {key}={value}")
    send(id=rid, result={"configOptions": config(sid)})


for line in sys.stdin:
    frame = json.loads(line)
    rid, method, params = frame.get("id"), frame.get("method"), frame.get("params", {})
    if method is None:
        continue
    if method == "initialize":
        send(id=rid, result={"protocolVersion": 1, "agentInfo": {"name": NAME, "version": "0.1"},
                             "agentCapabilities": {"loadSession": False,
                                                   "sessionCapabilities": {"close": {}}}})
    elif method == "session/new":
        sid = f"{NAME.lower()}-{next(counter)}"
        state[sid] = {"model": MODELS[0], "effort": "low", "mode": MODES[0] if MODES else None, "cwd": params.get("cwd"), "turns": 0}
        log(f"new {sid} cwd={params.get('cwd')}")
        result = {"sessionId": sid}
        if not BARE:
            result["configOptions"] = config(sid)
        send(id=rid, result=result)
    elif method == "session/set_config_option":
        threading.Thread(target=set_option, args=(rid, params), daemon=True).start()
    elif method == "session/prompt":
        sid = params["sessionId"]
        s = state[sid]
        s["turns"] += 1
        text = " ".join(b.get("text", "") for b in params["prompt"] if b.get("type") == "text")
        log(f"prompt {sid} {text!r} blocks={[b.get('type') for b in params['prompt']]}")
        if text.strip() == "status":
            say(sid, f"{NAME} turn {s['turns']} on {sid}: model {s['model']}, effort {s['effort']}, mode {s['mode']}, cwd {s['cwd']}.")
        else:
            say(sid, f"{NAME} read: {text}")
        send(id=rid, result={"stopReason": "end_turn"})
    elif method == "session/close":
        log(f"close {params.get('sessionId')}")
        send(id=rid, result={})
    elif method == "session/cancel":
        pass
    elif rid is not None:
        send(id=rid, error={"code": -32601, "message": f"{method} not supported"})
