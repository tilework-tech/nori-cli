#!/usr/bin/env python3
"""Synthetic stdio ACP agent server for attach replay verification ("kiln").

Independent of the #770 benchmark agent. All content is generated.

argv[1] is a control directory:
  sections     number of history sections to emit for the seed prompt
  native-load  if present, advertise loadSession and replay history on session/load
  live-N       a JSON [chunks, interval_ms] request; the agent streams one live turn
               per file (N increasing) and renames it to live-N.done
  ledger.jsonl every request and notable action, with monotonic_ns and pid

The seed prompt is any prompt starting with "kiln seed". Other prompts get the
reply "Kiln answer to <prompt>." so a scenario can prove which adapter answered.
"""
import json
import os
import sys
import threading
import time
from pathlib import Path

ROOT = Path(sys.argv[1])
LOCK = threading.Lock()

FIRST = "KilnOpening"
LATEST = "KilnLatestEnd"
FINAL_TOOL = "KilnFinalTool"
TAIL_LINES = 90


def mark(i):
    return f"KilnMark{i:04d}"


def ledger(event, **fields):
    with LOCK, (ROOT / "ledger.jsonl").open("a") as handle:
        handle.write(json.dumps({"monotonic_ns": time.monotonic_ns(), "pid": os.getpid(),
                                 "event": event, **fields}) + "\n")


def send(message):
    with LOCK:
        sys.stdout.write(json.dumps(message) + "\n")
        sys.stdout.flush()


def upd(sid, update_kind, **fields):
    return {"jsonrpc": "2.0", "method": "session/update",
            "params": {"sessionId": sid, "update": {"sessionUpdate": update_kind, **fields}}}


def text(sid, update_kind, mid, body):
    return upd(sid, update_kind, messageId=mid, content={"type": "text", "text": body})


def history(sid, sections):
    """Varied sections: prose, tables, lists, two code languages, plans, tool output."""
    for i in range(1, sections + 1):
        yield text(sid, "user_message_chunk", f"q-{i}", f"Check kiln batch {i} please.")
        lang = "python" if i % 2 else "rust"
        code = (f"def batch_{i}():\n    return {i} * 3\n" if lang == "python"
                else f"fn batch_{i}() -> u32 {{\n    {i} * 3\n}}\n")
        body = (("" if i > 1 else FIRST + "\n\n") +
                f"## {mark(i)}\n\n"
                f"Batch {i} fired at *cone {i % 12 + 1}*; glaze `celadon-{i}` held.\n\n"
                f"| shelf | temp |\n|---|---|\n| {i} | {1200 + i} |\n\n"
                f"1. Loaded\n2. Fired\n3. Cooled\n\n"
                f"```{lang}\n{code}```\n")
        step = 7 + i % 11
        for off in range(0, len(body), step):
            yield text(sid, "agent_message_chunk", f"a-{i}", body[off:off + step])
        if i % 5 == 0:
            yield upd(sid, "plan", entries=[
                {"content": f"Inspect batch {i}", "priority": "medium", "status": "completed"},
                {"content": f"Record batch {i}", "priority": "low", "status": "pending"}])
        tid = f"probe-{i}"
        yield upd(sid, "tool_call", toolCallId=tid, title=f"Read kiln-{i}.log", kind="read",
                  status="in_progress", rawInput={"path": f"kiln-{i}.log"})
        yield upd(sid, "tool_call_update", toolCallId=tid, status="completed", content=[
            {"type": "content", "content": {"type": "text",
             "text": "\n".join(f"kiln-{i} reading {r}" for r in range(1, 4))}}])
    tail = "\n".join(f"Closing note {n:03d} for the run." for n in range(1, TAIL_LINES + 1))
    tail = f"Final summary across {sections} batches.\n\n{tail}\n\n{LATEST}"
    for off in range(0, len(tail), 40):
        yield text(sid, "agent_message_chunk", "a-final", tail[off:off + 40])
    yield upd(sid, "tool_call", toolCallId="final-tool", title=FINAL_TOOL, kind="read",
              status="completed", rawInput={"path": "final.txt"})


def watch_live(sid):
    n = 1
    while True:
        req = ROOT / f"live-{n}"
        if req.exists():
            chunks, interval = json.loads(req.read_text())
            req.rename(ROOT / f"live-{n}.done")
            send(upd(sid, "session_info_update", _meta={"nori": {"status": "working"}}))
            for c in range(1, chunks):
                send(text(sid, "agent_message_chunk", f"live-{n}", f"tick{c} "))
                time.sleep(interval / 1000)
            send(text(sid, "agent_message_chunk", f"live-{n}", f"KilnLive{n:04d}"))
            send(upd(sid, "session_info_update", _meta={"nori": {"status": "idle"}}))
            ledger("live", n=n)
            n += 1
        time.sleep(0.002)


def main():
    native = (ROOT / "native-load").exists()
    for line in sys.stdin:
        req = json.loads(line)
        if "id" not in req or "method" not in req:
            continue
        method = req["method"]
        ledger("request", method=method)
        if method == "initialize":
            result = {"protocolVersion": 1, "agentCapabilities": {"loadSession": native},
                      "agentInfo": {"name": "Kiln", "version": "1"}}
        elif method == "session/new":
            result = {"sessionId": f"kiln-{os.getpid()}"}
        elif method == "session/load" and native:
            sid = req["params"]["sessionId"]
            count = 0
            for message in history(sid, int((ROOT / "sections").read_text())):
                send(message)
                count += 1
            ledger("loaded", updates=count)
            result = {}
        elif method == "session/prompt":
            sid = req["params"]["sessionId"]
            prompt = "".join(p.get("text", "") for p in req["params"]["prompt"])
            if prompt.startswith("kiln seed"):
                count = 0
                for message in history(sid, int((ROOT / "sections").read_text())):
                    send(message)
                    count += 1
                ledger("seeded", updates=count)
                threading.Thread(target=watch_live, args=(sid,), daemon=True).start()
            else:
                # A transcript-fallback resume prefixes its first prompt with a summary.
                asked = prompt.strip().splitlines()[-1].strip() if prompt.strip() else ""
                send(text(sid, "agent_message_chunk", f"r-{time.monotonic_ns()}",
                          f"Kiln answer to {asked}."))
                ledger("prompt", text=asked, prompt_bytes=len(prompt.encode()))
            result = {"stopReason": "end_turn"}
        elif method == "session/close":
            result = {}
        else:
            send({"jsonrpc": "2.0", "id": req["id"],
                  "error": {"code": -32601, "message": "unsupported"}})
            continue
        send({"jsonrpc": "2.0", "id": req["id"], "result": result})
        ledger("response", method=method)


if __name__ == "__main__":
    main()
