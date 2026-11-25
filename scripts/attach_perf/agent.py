#!/usr/bin/env python3
"""Synthetic ACP subprocess for the attach terminal benchmark."""

import json
import os
import sys
import threading
import time
from pathlib import Path

FIRST = "FirstReplayBeacon"
LAST = "FinalReplayBeacon"
FENCE = "CompleteReplayBeacon"
LIVE = "LiveReplayBeacon"
TITLE = "Replay benchmark target"


def turn_marker(index):
    return f"SectionBeacon{index:04d}"


def update(session, event_type, **fields):
    return {"jsonrpc": "2.0", "method": "session/update", "params": {
        "sessionId": session, "update": {"sessionUpdate": event_type, **fields}
    }}


def transcript(session, turns):
    """One synthetic restored conversation, with numbered user/agent/tool parts."""
    for number in range(1, turns + 1):
        yield update(session, "user_message_chunk", messageId=f"user-{number}",
                     content={"type": "text", "text": f"Inspect sample {number}."})
        text = (f"### {turn_marker(number)}\n\n"
                f"Read **sample {number}** and compare its measured range.\n\n"
                f"- Input: `example-{number}.dat`\n- Output: **accepted**\n\n"
                f"```rust\nlet sample = {number};\n```\n")
        if number == 1:
            text = FIRST + "\n\n" + text
        for chunk in range(0, len(text), 24):
            yield update(session, "agent_message_chunk", messageId=f"answer-{number}",
                         content={"type": "text", "text": text[chunk:chunk + 24]})
        yield update(session, "tool_call", toolCallId=f"read-{number}",
                     title=f"Read sample-{number}.dat", status="in_progress", kind="read",
                     rawInput={"path": f"sample-{number}.dat"})
        yield update(session, "tool_call_update", toolCallId=f"read-{number}", status="completed",
                     content=[{"type": "content", "content": {
                         "type": "text", "text": f"Observed range {number} through {number + 3}."}}])
    # Keep the final answer distinct from the last user-visible replay event.
    yield update(session, "agent_message_chunk", messageId="final-answer",
                 content={"type": "text", "text": f"Summary for {turns} inspected samples. {LAST}"})
    yield update(session, "tool_call", toolCallId="fence", title=FENCE,
                 status="completed", kind="read", rawInput={"path": f"{FENCE}.txt"})


def main():
    root = Path(sys.argv[1])
    log_path = root / "agent.jsonl"
    write_lock = threading.Lock()

    def log(event, **fields):
        with log_path.open("a") as log_file:
            print(json.dumps({"monotonic_ns": time.monotonic_ns(), "event": event,
                              "pid": os.getpid(), **fields}), file=log_file)

    def send(message):
        with write_lock:
            print(json.dumps(message), flush=True)

    def watch_live(session):
        last = 0
        while True:
            try:
                sequence, chunks, interval_ms = json.loads((root / "live").read_text())
            except FileNotFoundError:
                sequence = last
            if sequence > last:
                marker = f"{LIVE}{sequence:04d}"
                send(update(session, "session_info_update", _meta={"nori": {"status": "working"}}))
                for chunk in range(1, chunks):
                    send(update(session, "agent_message_chunk", messageId=f"live-{sequence}",
                                content={"type": "text", "text": f"step {chunk} "}))
                    time.sleep(interval_ms / 1000)
                send(update(session, "agent_message_chunk", messageId=f"live-{sequence}",
                            content={"type": "text", "text": marker}))
                send(update(session, "session_info_update", _meta={"nori": {"status": "idle"}}))
                log("live", marker=marker)
                last = sequence
            time.sleep(.003)

    for line in sys.stdin:
        request = json.loads(line)
        if "id" not in request or "method" not in request:
            continue
        method = request["method"]
        log("request", method=method)
        if method == "initialize":
            result = {"protocolVersion": 1, "agentCapabilities": {"loadSession": (root / "native-load").exists()},
                      "agentInfo": {"name": "Synthetic", "version": "1"}}
        elif method == "session/new":
            result = {"sessionId": f"synthetic-{os.getpid()}"}
        elif method == "session/load" and (root / "native-load").exists():
            for message in transcript(request["params"]["sessionId"], int((root / "turns").read_text())):
                send(message)
            result = {}
        elif method == "session/prompt":
            session = request["params"]["sessionId"]
            prompt = "".join(item.get("text", "") for item in request["params"]["prompt"])
            if prompt == TITLE:
                turns = int((root / "turns").read_text())
                count = 0
                for message in transcript(session, turns):
                    send(message)
                    count += 1
                log("history_sent", updates=count, turns=turns)
                threading.Thread(target=watch_live, args=(session,), daemon=True).start()
            else:
                send(update(session, "agent_message_chunk",
                            content={"type": "text", "text": "Synthetic acknowledgement."}))
                log("prompt", text=prompt)
            result = {"stopReason": "end_turn"}
        elif method == "session/close":
            result = {}
        else:
            send({"jsonrpc": "2.0", "id": request["id"],
                  "error": {"code": -32601, "message": "Unsupported benchmark method"}})
            continue
        send({"jsonrpc": "2.0", "id": request["id"], "result": result})
        log("response", method=method)


if __name__ == "__main__":
    main()
