"""Trace one synthetic adapter connection and require its own saved ID on load."""
import fcntl
import json
import os
import sys
import time
from pathlib import Path

name, trace_dir = sys.argv[1], Path(sys.argv[2])
pid = os.getpid()
lease = (trace_dir / str(pid)).open("w")
fcntl.flock(lease, fcntl.LOCK_EX)
saved = name + "-saved"
active = None
for line in sys.stdin:
    msg = json.loads(line)
    method, params = msg["method"], msg.get("params", {})
    sid = params.get("sessionId")
    with (trace_dir / "traffic").open("a") as trace:
        print(json.dumps([pid, name, method, sid, os.getcwd(), params.get("cwd")]), file=trace)
    reply = {}
    if method == "initialize":
        result = {"protocolVersion": 1, "agentCapabilities": {
            "loadSession": name not in ("list-only", "resume-only"),
            "sessionCapabilities": {"list": {}, **({"resume": {}} if name == "resume-only" else {})}}}
    elif method == "session/list":
        if name == "waiting":
            while True:
                time.sleep(1)
        if name == "broken":
            reply["error"] = {"code": -32040, "message": "catalog unavailable"}
        result = {"sessions": [{"sessionId": saved, "cwd": os.getcwd(),
                               "_meta": {"sample": "picker"}}]}
    elif method == "session/new":
        active = name + "-new"
        result = {"sessionId": active}
    elif method in ("session/load", "session/resume") and sid == saved:
        if name == "declines":
            print(json.dumps({"jsonrpc": "2.0", "id": msg["id"], "error": {
                "code": -32041, "message": "restore declined"}}), flush=True)
            continue
        active = sid
        if name not in ("empty", "resume-only"):
            for index in range(64):
                print(json.dumps({"jsonrpc": "2.0", "method": "session/update", "params": {
                    "sessionId": sid, "update": {"sessionUpdate": "agent_message_chunk",
                        "content": {"type": "text", "text": f"saved history {index}"}}}}), flush=True)
        result = {}
    elif method == "session/prompt" and sid == active:
        result = {"stopReason": "end_turn"}
    else:
        reply["error"] = {"code": -32602, "message": "unexpected adapter request"}
    if "error" not in reply:
        reply["result"] = result
    print(json.dumps({"jsonrpc": "2.0", "id": msg["id"], **reply}), flush=True)
