#!/usr/bin/env python3
"""Synthetic ACP agent: replays tool-call updates from a JSON scenario file.

Usage: scripted_acp_agent.py SCENARIO.json
The scenario is a list of steps. Each step is {"update": {...sessionUpdate...}}
or {"sleep": seconds} or {"text": "agent message chunk"}.
Paths may contain "{cwd}", replaced with the session cwd.
"""
import json, sys, time

scenario_path = sys.argv[1]
session_cwd = None


def send(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()


def subst(value):
    if isinstance(value, str):
        return value.replace("{cwd}", session_cwd or "")
    if isinstance(value, list):
        return [subst(v) for v in value]
    if isinstance(value, dict):
        return {k: subst(v) for k, v in value.items()}
    return value


def notify(session_id, update):
    send({"jsonrpc": "2.0", "method": "session/update",
          "params": {"sessionId": session_id, "update": update}})


for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    msg = json.loads(line)
    method, mid = msg.get("method"), msg.get("id")
    if mid is None:
        continue  # notification (e.g. session/cancel)
    if method == "initialize":
        send({"jsonrpc": "2.0", "id": mid, "result": {
            "protocolVersion": 1,
            "agentCapabilities": {"loadSession": False},
            "authMethods": [],
            "agentInfo": {"name": "synthetic-explorer", "version": "0.0.1"}}})
    elif method == "session/new":
        session_cwd = msg["params"].get("cwd")
        send({"jsonrpc": "2.0", "id": mid, "result": {"sessionId": "synthetic-session-1"}})
    elif method == "session/prompt":
        sid = msg["params"]["sessionId"]
        with open(scenario_path) as f:
            steps = json.load(f)
        for step in steps:
            if "sleep" in step:
                time.sleep(step["sleep"])
            elif "text" in step:
                notify(sid, {"sessionUpdate": "agent_message_chunk",
                             "content": {"type": "text", "text": step["text"]}})
            else:
                notify(sid, subst(step["update"]))
                time.sleep(0.02)
        send({"jsonrpc": "2.0", "id": mid, "result": {"stopReason": "end_turn"}})
    else:
        send({"jsonrpc": "2.0", "id": mid, "result": {}})
