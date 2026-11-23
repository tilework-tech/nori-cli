#!/usr/bin/env python3
"""Synthetic ACP agent for PR 734 verification. All data is made up."""
import json, sys, os

def send(msg):
    sys.stdout.write(json.dumps(msg) + "\n"); sys.stdout.flush()

def update(sid, u):
    send({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": sid, "update": u}})

def text(t):
    return [{"type": "content", "content": {"type": "text", "text": t}}]

D = int(os.environ.get("SYNTH_DIFF_LINES", "20"))
N = int(os.environ.get("SYNTH_LINES", "12"))
CMD_OUT = "\n".join(f"cmd line {i:02d} of {N:02d}" for i in range(1, N + 1))
GEN_OUT = "\n".join(f"fetched row {i:02d}" for i in range(1, 9))
SEARCH_OUT = "\n".join(f"src/orbit.rs:{i}: match {i}" for i in range(1, 9))
OLD = "\n".join(f"let v{i} = {i};" for i in range(1, D + 1)) + "\n"
NEW = "\n".join(f"let v{i} = {i * 10};" for i in range(1, D + 1)) + "\n"

def turn(sid):
    scen = os.environ.get("SYNTH_SCENARIO", "all").split(",")
    scen = ["all"] if "all" in scen else scen
    if "all" in scen or "cmd" in scen:
        update(sid, {"sessionUpdate": "tool_call", "toolCallId": "c1", "title": "Run the orbit suite",
                     "kind": "execute", "status": "in_progress",
                     "rawInput": {"command": "cargo test -p orbit-demo"}})
        update(sid, {"sessionUpdate": "tool_call_update", "toolCallId": "c1", "status": "completed",
                     "content": text(CMD_OUT), "rawOutput": {"exit_code": 0}})
    if "all" in scen or "generic" in scen:
        update(sid, {"sessionUpdate": "tool_call", "toolCallId": "g1", "title": "Fetch release notes",
                     "kind": "fetch", "status": "in_progress",
                     "rawInput": {"url": "https://example.test/notes"}})
        update(sid, {"sessionUpdate": "tool_call_update", "toolCallId": "g1", "status": "completed",
                     "content": text(GEN_OUT)})
    if "all" in scen or "search" in scen:
        update(sid, {"sessionUpdate": "tool_call", "toolCallId": "s1", "title": "Search for match",
                     "kind": "search", "status": "in_progress", "rawInput": {"pattern": "match"}})
        update(sid, {"sessionUpdate": "tool_call_update", "toolCallId": "s1", "status": "completed",
                     "content": text(SEARCH_OUT)})
    if "read" in scen:
        rpath = os.path.join(os.getcwd(), "orbit.rs")
        update(sid, {"sessionUpdate": "tool_call", "toolCallId": "r1", "title": "Read orbit.rs",
                     "kind": "read", "status": "in_progress", "rawInput": {"file_path": rpath},
                     "locations": [{"path": rpath}]})
        update(sid, {"sessionUpdate": "tool_call_update", "toolCallId": "r1", "status": "completed",
                     "content": text(GEN_OUT.replace("fetched", "read"))})
    if "all" in scen or "diff" in scen:
        path = os.path.join(os.getcwd(), "orbit.rs")
        update(sid, {"sessionUpdate": "tool_call", "toolCallId": "e1", "title": "Edit orbit.rs",
                     "kind": "edit", "status": "in_progress",
                     "content": [{"type": "diff", "path": path, "oldText": OLD, "newText": NEW}],
                     "locations": [{"path": path}]})
        update(sid, {"sessionUpdate": "tool_call_update", "toolCallId": "e1", "status": "completed"})
    update(sid, {"sessionUpdate": "agent_message_chunk",
                 "content": {"type": "text", "text": "Synthetic turn finished."}})

for raw in sys.stdin:
    raw = raw.strip()
    if not raw:
        continue
    msg = json.loads(raw)
    mid, method = msg.get("id"), msg.get("method")
    if mid is None:
        continue
    if method == "initialize":
        res = {"protocolVersion": 1, "agentCapabilities": {"loadSession": False}, "authMethods": []}
    elif method == "session/new":
        res = {"sessionId": "synthetic-session-1"}
    elif method == "session/prompt":
        turn(msg["params"]["sessionId"]); res = {"stopReason": "end_turn"}
    elif method == "session/close":
        res = {}
    else:
        send({"jsonrpc": "2.0", "id": mid, "error": {"code": -32601, "message": "method not found"}}); continue
    send({"jsonrpc": "2.0", "id": mid, "result": res})
