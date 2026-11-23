"""Start two synthetic turns without receiving session/prompt."""
import json
import sys
from pathlib import Path

cancels = 0
listing = None


def send(**fields):
    print(json.dumps({"jsonrpc": "2.0", **fields}), flush=True)


def status(turn, state, **extra):
    meta = {"nori/turnId": turn, "nori/turnState": state, **extra}
    send(method="session/update", params={
        "sessionId": "trail", "update": {"sessionUpdate": "session_info_update", "_meta": meta}
    })


for line in sys.stdin:
    frame = json.loads(line)
    method = frame.get("method")
    if method == "initialize":
        send(id=frame["id"], result={"protocolVersion": 1, "agentCapabilities": {"loadSession": True}})
    elif method == "session/load":
        status("historic", "active")
        send(id=frame["id"], result={})
        status("north", "active")
    elif method == "session/new":
        status("north", "active")
        send(id=frame["id"], result={"sessionId": "trail"})
    elif method == "session/list":
        listing = frame["id"]
        Path("listing").touch()
    elif method == "session/cancel":
        with open("cancels", "a", encoding="utf-8") as log:
            print(frame["params"]["sessionId"], file=log)
        cancels += 1
        if listing is not None:
            send(id=listing, result={"sessions": []})
            listing = None
        if cancels == 1:
            status("north", "idle", **{"nori/stopReason": "cancelled"})
            status("south", "active")
    elif "id" in frame:
        send(id=frame["id"], error={"code": -32601, "message": "Unknown method"})
