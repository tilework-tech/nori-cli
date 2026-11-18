"""Deterministic stdio ACP peer. Every process allocates session IDs from zero."""
import itertools
import json
import sys

name = sys.argv[1]
ids = itertools.count()
permissions = itertools.count(40)
pending = {}
sessions = set()


def send(**fields):
    print(json.dumps({"jsonrpc": "2.0", **fields}), flush=True)


def update(sid, kind, text):
    send(method="session/update", params={
        "sessionId": sid, "update": {
            "sessionUpdate": kind, "content": {"type": "text", "text": text}
        }
    })


def options(value="slow"):
    return [{"type": "select", "id": "speed", "name": "Speed",
             "currentValue": value, "options": [
                 {"value": v, "name": v.title()} for v in ("slow", "fast")
             ]}]


for line in sys.stdin:
    frame = json.loads(line)
    rid = frame.get("id")
    method = frame.get("method")
    params = frame.get("params", {})
    sid = params.get("sessionId")

    if method is None:
        if rid in pending:
            prompt, sid, metadata = pending.pop(rid)
            decision = frame["result"]["outcome"]
            update(sid, "agent_message_chunk", f"{name}/{sid}/{decision['outcome']}")
            send(id=prompt, result={"stopReason": "end_turn", "_meta": {"decision": decision, "receivedPromptMeta": metadata}})
    elif method == "initialize":
        caps = {"close": {}, "resume": {}}
        if name == "orchid":
            caps["fork"] = {}
        send(id=rid, result={"protocolVersion": 1, "agentInfo": {"name": name, "version": "1"},
                             "agentCapabilities": {"loadSession": True,
                                                   "mcpCapabilities": {"http": True},
                                                   "sessionCapabilities": caps}, "_meta": {"receivedClient": params}})
    elif method == "session/new":
        sid = str(next(ids))
        sessions.add(sid)
        send(id=rid, result={"sessionId": sid,
             "modes": {"currentModeId": "explore",
                       "availableModes": [{"id": "explore", "name": "Explore"}]},
             "models": {"currentModelId": name,
                        "availableModels": [{"modelId": name, "name": name}]},
             "configOptions": options(), "_meta": {"mcpServers": params["mcpServers"]}})
    elif method == "session/cancel":
        for permission, (prompt, session, _) in list(pending.items()):
            if session == sid:
                del pending[permission]
                send(id=prompt, result={"stopReason": "cancelled"})
    elif method in ("session/load", "session/resume"):
        sessions.add(sid)
        if method == "session/load":
            update(sid, "user_message_chunk", "saved input")
        send(id=rid, result={"configOptions": options(), "_meta": {"fixture": name}})
    elif sid not in sessions:
        send(id=rid, error={"code": -32002, "message": "unknown session", "data": {"sid": sid}})
    elif method == "session/prompt":
        text = params["prompt"][0]["text"]
        update(sid, "user_message_chunk", text)
        if text == "finish":
            send(id=rid, result={"stopReason": "end_turn"})
            continue
        perm = next(permissions)
        pending[perm] = (rid, sid, params.get("_meta"))
        send(id=perm, method="session/request_permission", params={
            "sessionId": sid, "toolCall": {"toolCallId": sid, "title": name},
            "options": [{"optionId": f"allow-{name}", "name": "Once", "kind": "allow_once"}]})
    elif method == "session/fork":
        # Always accepts if called, so the client can detect a host forwarding
        # this operation to an agent that never advertised it.
        child = str(next(ids))
        sessions.add(child)
        send(id=rid, result={"sessionId": child, "_meta": {"parent": sid}})
    elif method == "session/set_config_option":
        send(id=rid, result={"configOptions": options(params["value"])})
    elif method == "session/set_mode":
        if params["modeId"] == "explore":
            send(id=rid, result={"_meta": {"mode": "explore"}})
        else:
            send(id=rid, error={"code": -32020, "message": "unavailable mode",
                                "data": {"requested": params["modeId"], "agent": name}})
    elif method == "session/close":
        sessions.remove(sid)
        send(id=rid, result={"_meta": {"closed": sid}})
    else:
        send(id=rid, error={"code": -32601, "message": "unknown method"})
