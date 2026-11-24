#!/usr/bin/env python3
"""Synthetic stdio ACP agent server for host TUI acceptance runs.

Every frame the host sends is appended to the ledger named by LANTERN_LEDGER,
so a scenario can prove what reached the adapter, independent of the screen.

Environment:
  LANTERN_NAME     agentInfo name and ledger tag (default "lantern")
  LANTERN_LEDGER   path of the JSONL ledger (required)
  LANTERN_MODES    comma-separated mode ids, first is the starting mode
  LANTERN_SAVED    JSON file with a list of saved sessions {sessionId,cwd,title}

Prompt words (first word of the prompt text):
  hold     stream a tick every 200 ms until cancelled (records ticks)
  think    send a thought chunk, then a reply
  scan     send a read tool call with a line range and a search with hits
  bulky    send a tool call whose output has 60 numbered lines
  perm     ask for permission before replying
  anything else: reply "<name> heard: <text>"
"""
import json
import os
import sys
import threading
import time

NAME = os.environ.get("LANTERN_NAME", "lantern")
LEDGER = os.environ["LANTERN_LEDGER"]
MODES = [m for m in os.environ.get("LANTERN_MODES", "").split(",") if m]
SAVED_PATH = os.environ.get("LANTERN_SAVED")

lock = threading.Lock()
sessions = {}
cancelled = set()
pending_permission = {}
next_session = [0]
next_request = [9000]


def ledger(kind, **data):
    with lock, open(LEDGER, "a") as handle:
        handle.write(json.dumps({"t": time.time(), "adapter": NAME, "pid": os.getpid(),
                                 "kind": kind, **data}) + "\n")


def send(**fields):
    with lock:
        sys.stdout.write(json.dumps({"jsonrpc": "2.0", **fields}) + "\n")
        sys.stdout.flush()


def update(sid, body):
    send(method="session/update", params={"sessionId": sid, "update": body})


def chunk(sid, kind, text):
    update(sid, {"sessionUpdate": kind, "content": {"type": "text", "text": text}})


def config_options(sid):
    return [{"id": "mode", "name": "Mode", "category": "mode", "type": "select",
             "currentValue": sessions[sid]["mode"],
             "options": [{"value": m, "name": m} for m in MODES]}]


def mode_state(sid):
    if not MODES:
        return {}
    return {"configOptions": config_options(sid)}


def saved_sessions():
    if not SAVED_PATH or not os.path.exists(SAVED_PATH):
        return []
    with open(SAVED_PATH) as handle:
        return json.load(handle)


def run_prompt(rid, sid, text):
    lines = [l.strip() for l in text.splitlines() if l.strip()]
    text = lines[-1] if lines else ""
    word = text.split()[0] if text.split() else ""
    if word == "hold":
        ticks = 0
        while sid not in cancelled:
            ticks += 1
            chunk(sid, "agent_message_chunk", f"tick {ticks} ")
            ledger("tick", sessionId=sid, n=ticks)
            time.sleep(0.2)
        cancelled.discard(sid)
        ledger("turn_end", sessionId=sid, stopReason="cancelled", ticks=ticks)
        send(id=rid, result={"stopReason": "cancelled"})
        return
    if word == "think":
        chunk(sid, "agent_thought_chunk", "**Weighing the lighthouse log** before answering.")
        time.sleep(0.3)
    if word == "scan":
        update(sid, {"sessionUpdate": "tool_call", "toolCallId": "scan-1", "kind": "read",
                     "title": "Read harbor/tides.md", "status": "completed",
                     "locations": [{"path": "harbor/tides.md", "line": 12}],
                     "rawInput": {"path": "harbor/tides.md", "offset": 12, "limit": 8}})
        update(sid, {"sessionUpdate": "tool_call", "toolCallId": "scan-2", "kind": "search",
                     "title": "Search for beacon", "status": "completed",
                     "rawInput": {"pattern": "beacon", "output_mode": "count"},
                     "_meta": {"claudeCode": {"toolName": "Grep", "toolResponse": {
                         "mode": "count", "numMatches": 3, "numFiles": 2}}}})
    if word == "bulky":
        body = "\n".join(f"row {n:02d} of the synthetic ledger" for n in range(1, 61))
        update(sid, {"sessionUpdate": "tool_call", "toolCallId": "bulky-1", "kind": "execute",
                     "title": "count rows", "status": "completed",
                     "content": [{"type": "content", "content": {"type": "text", "text": body}}]})
    if word == "perm":
        pid = next_request[0]
        next_request[0] += 1
        pending_permission[pid] = (rid, sid)
        send(id=pid, method="session/request_permission", params={
            "sessionId": sid,
            "toolCall": {"toolCallId": f"perm-{pid}", "title": "Paint the boathouse", "kind": "edit"},
            "options": [{"optionId": "allow", "name": "Allow once", "kind": "allow_once"},
                        {"optionId": "reject", "name": "Reject", "kind": "reject_once"}]})
        return
    chunk(sid, "agent_message_chunk", f"{NAME} heard: {text}")
    ledger("turn_end", sessionId=sid, stopReason="end_turn")
    send(id=rid, result={"stopReason": "end_turn"})


def main():
    ledger("spawn", cwd=os.getcwd(), argv=sys.argv[1:])
    for line in sys.stdin:
        frame = json.loads(line)
        rid, method = frame.get("id"), frame.get("method")
        params = frame.get("params") or {}
        sid = params.get("sessionId")
        if method is None:
            ledger("response", id=rid, body=frame.get("result") or frame.get("error"))
            if rid in pending_permission:
                prompt_id, psid = pending_permission.pop(rid)
                outcome = (frame.get("result") or {}).get("outcome", {})
                chunk(psid, "agent_message_chunk", f"{NAME} permission: {json.dumps(outcome)}")
                send(id=prompt_id, result={"stopReason": "end_turn"})
            continue
        summary = {k: v for k, v in params.items() if k in ("cwd", "sessionId", "modeId", "configId", "value")}
        if method == "session/prompt":
            summary["text"] = "".join(b.get("text", "") for b in params.get("prompt", []) if b.get("type") == "text")
        ledger("request", method=method, **summary)
        if method == "initialize":
            send(id=rid, result={"protocolVersion": 1, "agentInfo": {"name": NAME, "version": "0.0.1"},
                                 "agentCapabilities": {"loadSession": True,
                                                       "sessionCapabilities": {"close": {}, "resume": {}, "list": {}}}})
        elif method == "session/new":
            sid = f"{NAME}-{next_session[0]}"
            next_session[0] += 1
            sessions[sid] = {"cwd": params.get("cwd"), "mode": MODES[0] if MODES else None}
            send(id=rid, result={"sessionId": sid, **mode_state(sid)})
        elif method == "session/list":
            send(id=rid, result={"sessions": saved_sessions()})
        elif method in ("session/load", "session/resume"):
            saved = {s["sessionId"]: s for s in saved_sessions()}
            sessions.setdefault(sid, {"cwd": params.get("cwd") or saved.get(sid, {}).get("cwd"),
                                      "mode": MODES[0] if MODES else None})
            if method == "session/load":
                chunk(sid, "user_message_chunk", "an earlier synthetic question")
                chunk(sid, "agent_message_chunk", "an earlier synthetic answer")
            send(id=rid, result=mode_state(sid))
        elif method == "session/close":
            sessions.pop(sid, None)
            send(id=rid, result={})
        elif method == "session/cancel":
            cancelled.add(sid)
            for pid, (prompt_id, psid) in list(pending_permission.items()):
                if psid == sid:
                    del pending_permission[pid]
                    send(id=prompt_id, result={"stopReason": "cancelled"})
        elif method == "session/set_mode":
            if sid in sessions:
                sessions[sid]["mode"] = params.get("modeId")
            send(id=rid, result={})
            update(sid, {"sessionUpdate": "current_mode_update", "currentModeId": params.get("modeId")})
        elif method == "session/set_config_option":
            if params.get("configId") == "mode" and sid in sessions and params.get("value") in MODES:
                sessions[sid]["mode"] = params["value"]
            send(id=rid, result={"configOptions": config_options(sid) if MODES and sid in sessions else []})
        elif method == "session/prompt":
            if sid not in sessions:
                send(id=rid, error={"code": -32002, "message": "unknown session"})
                continue
            threading.Thread(target=run_prompt, args=(rid, sid, summary["text"]), daemon=True).start()
        elif rid is not None:
            send(id=rid, error={"code": -32601, "message": f"{method} not offered by {NAME}"})
    ledger("stdin_eof")


if __name__ == "__main__":
    main()
