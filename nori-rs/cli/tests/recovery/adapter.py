#!/usr/bin/python3
"""An ACP fixture with one countable side effect per prompt."""
import json
import os
import pathlib
import sys
import time
import uuid


sessions = set()


def send(**message):
    print(json.dumps({"jsonrpc": "2.0", **message}), flush=True)


def handle(request):
    params = request.get("params", {})
    method = request.get("method")
    if method == "initialize":
        return {"protocolVersion": 1, "agentCapabilities": {"loadSession": not pathlib.Path("no-load").exists()}}
    if method == "session/new":
        session = str(uuid.uuid4())
        sessions.add(session)
        return {"sessionId": session}
    if method == "session/load":
        assert not pathlib.Path("no-load").exists()
        sessions.add(params["sessionId"])
        return {}
    if method == "session/prompt":
        assert params["sessionId"] in sessions
        text = params["prompt"][0]["text"]
        with open("executions", "a", encoding="utf-8") as log:
            log.write(text + "\n")
            log.flush()
            os.fsync(log.fileno())
        send(method="session/update", params={
            "sessionId": params["sessionId"],
            "update": {
                "sessionUpdate": "agent_message_chunk",
                "content": {"type": "text", "text": text},
            },
        })
        if text == "fill-journal":
            for _ in range(17):
                send(method="session/update", params={
                    "sessionId": params["sessionId"],
                    "update": {"sessionUpdate": "agent_message_chunk",
                               "content": {"type": "text", "text": "x" * 1024 ** 2}},
                })
        if text == "hold":
            deadline = time.monotonic() + 35
            while not pathlib.Path("unblock").exists():
                if time.monotonic() > deadline or os.getppid() == 1:
                    sys.exit(0)
                time.sleep(0.02)
        return {"stopReason": "end_turn"}
    return {}


for line in sys.stdin:
    message = json.loads(line)
    if "id" in message and "method" in message:
        send(id=message["id"], result=handle(message))
        if message.get("method") == "session/prompt" and message["params"]["prompt"][0]["text"] == "malformed-adapter-tail":
            print("{", flush=True)
