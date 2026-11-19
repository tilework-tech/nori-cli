"""Hold an ACP prompt open until a guarded steer races its completion."""
import json
import sys
import threading
import time
from pathlib import Path

owner = None
version = Path("version").read_text()


def delayed_response(request_id, result):
    while not Path("release").exists():
        time.sleep(0.01)
    respond(request_id, result)



def respond(request_id, result):
    print(json.dumps({"jsonrpc": "2.0", "id": request_id, "result": result}), flush=True)


def observed(kind, text):
    with open("observed", "a", encoding="utf-8") as log:
        print(kind, text, file=log)


for line in sys.stdin:
    frame = json.loads(line)
    method = frame.get("method")
    params = frame.get("params", {})
    if not method and frame.get("id") == "permission":
        observed("decision", frame["result"]["outcome"]["outcome"])
    elif method == "initialize":
        respond(frame["id"], {
            "protocolVersion": 1, "agentCapabilities": {},
            "agentInfo": {"name": "@agentclientprotocol/claude-agent-acp", "version": version},
            "_meta": {"steering": {"supported": True}},
        })
    elif method == "session/new":
        respond(frame["id"], {"sessionId": "fixture"})
    elif method == "session/prompt":
        text = params["prompt"][-1]["text"]
        observed("prompt", text)
        if text in ("original", "at boundary"):
            owner = frame["id"]
        elif text == "wait for release":
            threading.Thread(target=delayed_response,
                             args=(frame["id"], {"stopReason": "end_turn"}), daemon=True).start()
        else:
            respond(frame["id"], {"stopReason": "end_turn"})
    elif method == "_session/steering":
        assert params["sessionId"] == "fixture"
        assert params["_meta"]["steering"]["idleBehavior"] == "promptRequired"
        text = params["prompt"][-1]["text"]
        observed("steer", text)
        if text == "at boundary":
            print(json.dumps({"jsonrpc": "2.0", "id": "permission", "method": "session/request_permission",
                              "params": {"sessionId": "fixture", "toolCall": {"toolCallId": "synthetic-write"},
                                         "options": [{"optionId": "deny", "name": "Deny", "kind": "reject_once"}]}}), flush=True)
            respond(owner, {"stopReason": "end_turn"})
            respond(frame["id"], {"outcome": "promptRequired", "reason": "noRunningTurn"})
        elif text in ("no receipt", "late receipt"):
            respond(owner, {"stopReason": "end_turn"})
            if text == "late receipt":
                threading.Thread(target=delayed_response,
                                 args=(frame["id"], {"outcome": "injected"}), daemon=True).start()
        elif text == "stream early":
            print(json.dumps({"jsonrpc": "2.0", "method": "session/update",
                            "params": {"sessionId": "fixture", "update": {"sessionUpdate": "agent_message_chunk",
                                      "content": {"type":"text", "text":"agent observed correction"}}}}), flush=True)
            time.sleep(0.05)
            respond(frame["id"], {"outcome": "injected"})
        elif text == "refuse":
            respond(frame["id"], {"outcome": "failed"})
        elif text == "unexpected result":
            respond(frame["id"], {})
        else:
            respond(frame["id"], {"outcome": "injected"})
    elif method and "id" in frame:
        print(json.dumps({"jsonrpc": "2.0", "id": frame["id"],
                          "error": {"code": -32601, "message": "Method not found"}}), flush=True)
