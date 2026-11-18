"""Stateful synthetic ACP adapter with independent, enforced session modes."""
import fcntl
import itertools
import json
import os
import sys

profile = sys.argv[1]
advertised_mode = "bypassPermissions" if profile == "bypass-mode" else "inspect"
ids = itertools.count(1)
permissions = itertools.count(50)
sessions = {}
pending = {}
late_fork = None
deferred_closes = {}

if profile == "shutdown":
    process_lock = open("adapter.lock", "w")
    fcntl.flock(process_lock, fcntl.LOCK_EX)


def send(**fields):
    print(json.dumps({"jsonrpc": "2.0", **fields}), flush=True)


def modes(mode):
    available = ["edit"] if profile == "missing-mode" else [advertised_mode, "edit"]
    return {"currentModeId": mode, "availableModes": [
        {"id": m, "name": m} for m in available
    ]}


def cancel_pending(sid):
    for key, (prompt, owner, _) in list(pending.items()):
        if owner == sid:
            del pending[key]
            send(id=prompt, result={"stopReason": "cancelled"})


for line in sys.stdin:
    msg = json.loads(line)
    rid, method = msg.get("id"), msg.get("method")
    p = msg.get("params", {})
    sid = p.get("sessionId")
    if method is None:
        if rid in pending:
            prompt, owner, thread = pending.pop(rid)
            send(id=prompt, result={"stopReason": "end_turn", "_meta": {
                "thread": thread, "decision": msg.get("result")}})
            if owner in deferred_closes:
                send(id=deferred_closes.pop(owner), error={
                    "code": -32031, "message": "permission pending", "data": {"retry": True}
                })
        continue
    if method == "initialize":
        caps = {"close": {}, "fork": {}}
        meta = {"nori.dev/readOnlyFork": {"version": 1, "modeId": advertised_mode}}
        if profile == "uncertified":
            meta = {}
        if profile == "no-fork":
            del caps["fork"]
        if profile == "no-close":
            del caps["close"]
        result = {"protocolVersion": 1, "agentCapabilities": {
            "sessionCapabilities": caps, "_meta": meta}}
    elif method == "session/new":
        sid = str(next(ids))
        sessions[sid] = {"mode": "edit", "history": []}
        result = {"sessionId": sid, "modes": modes("edit")}
    elif method == "session/list":
        result = {"sessions": [{"sessionId": i, "cwd": os.getcwd()} for i in sessions]}
    elif method == "_fixture/active":
        result = {"sessions": sorted(sessions)}
    elif sid not in sessions:
        send(id=rid, error={"code": -32602, "message": "unknown fixture session"})
        continue
    elif method == "session/fork":
        if profile == "alias-parent":
            send(id=rid, result={"sessionId": sid, "modes": modes("edit")})
            continue
        if profile == "alias-sibling" and len(sessions) > 1:
            existing = next(i for i in sessions if i != sid)
            send(id=rid, result={"sessionId": existing, "modes": modes(sessions[existing]["mode"])})
            continue
        child = str(next(ids))
        sessions[child] = {"mode": "edit", "history": sessions[sid]["history"].copy()}
        result = {"sessionId": child, "modes": modes("edit")}
        if profile == "late-fork":
            late_fork = (rid, result)
            send(method="session/update", params={"sessionId": sid, "update": {
                "sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "fork started"}
            }})
            continue
    elif method == "session/set_mode":
        if profile == "reject-mode":
            send(id=rid, error={"code": -32602, "message": "mode unavailable"})
            continue
        sessions[sid]["mode"] = p["modeId"]
        result = {}
    elif method == "session/prompt":
        word = p["prompt"][0]["text"]
        thread = sessions[sid]
        if word == "drift":
            thread["mode"] = "edit"
            send(method="session/update", params={
                "sessionId": sid, "update": {"sessionUpdate": "current_mode_update", "currentModeId": "edit"}
            })
            send(id=rid, result={"stopReason": "end_turn"})
            continue
        if word.startswith("hold"):
            key = next(permissions)
            pending[key] = (rid, sid, word)
            send(id=key, method="session/request_permission", params={
                "sessionId": sid, "toolCall": {"toolCallId": word, "kind": "read"},
                "options": [{"optionId": word, "kind": "allow_once", "name": "Once"}]})
            continue
        if word == "write" and thread["mode"] != "inspect":
            with open("fixture-write", "w") as marker:
                marker.write(word)
        thread["history"].append(word)
        send(method="session/update", params={"sessionId": sid, "update": {
            "sessionUpdate": "agent_message_chunk", "content": {
                "type": "text", "text": "|".join(thread["history"])}}})
        result = {"stopReason": "end_turn", "_meta": {"pid": os.getpid(), "mode": thread["mode"]}}
    elif method == "session/close":
        if profile == "close-retry" and any(owner == sid for _, owner, _ in pending.values()):
            if any(owner == sid and word == "hold-retry" for _, owner, word in pending.values()):
                deferred_closes[sid] = rid
                continue
            send(id=rid, error={"code": -32031, "message": "permission pending", "data": {"retry": True}})
            continue
        cancel_pending(sid)
        del sessions[sid]
        send(id=rid, result={})
        if late_fork:
            request, response = late_fork
            late_fork = None
            send(id=request, result=response)
        continue
    elif method == "session/cancel":
        cancel_pending(sid)
        continue
    else:
        send(id=rid, error={"code": -32601, "message": "unsupported fixture method"})
        continue
    send(id=rid, result=result)
