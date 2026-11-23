#!/usr/bin/env python3
"""Synthetic ACP agent for verifying thought text in history (nori-cli #733).

Each prompt picks a scenario by keyword on its last line. All text is synthetic.
SYNTH_LOAD=1 advertises loadSession and replays recorded updates on session/load,
so the client's live-delta replay path is exercised. Without it the client falls
back to its own transcript replay.
"""
import json, os, sys, time, uuid

LOAD = os.environ.get("SYNTH_LOAD") == "1"
LOG_DIR = os.environ.get("SYNTH_LOG_DIR", "/tmp/synth-log")
os.makedirs(LOG_DIR, exist_ok=True)
session_id = None
tool_no = 0


def send(obj):
    sys.stdout.write(json.dumps(obj) + "\n"); sys.stdout.flush()


def update(u, record=True):
    send({"jsonrpc": "2.0", "method": "session/update",
          "params": {"sessionId": session_id, "update": u}})
    if record:
        with open(os.path.join(LOG_DIR, session_id + ".jsonl"), "a") as f:
            f.write(json.dumps(u) + "\n")
    time.sleep(0.02)


def thought(t): update({"sessionUpdate": "agent_thought_chunk", "content": {"type": "text", "text": t}})
def answer(t): update({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": t}})


def tool(title):
    global tool_no
    tool_no += 1
    tid = f"synth-tool-{tool_no}"
    update({"sessionUpdate": "tool_call", "toolCallId": tid, "title": title, "kind": "read", "status": "in_progress"})
    update({"sessionUpdate": "tool_call_update", "toolCallId": tid, "status": "completed",
            "content": [{"type": "content", "content": {"type": "text", "text": "synthetic output"}}]})


def pause(label):
    gate = os.environ.get("SYNTH_GATE_DIR")
    if not gate:
        return
    os.makedirs(gate, exist_ok=True)
    with open(os.path.join(gate, "waiting"), "w") as f:
        f.write(label + "\n")
    path = os.path.join(gate, label)
    while not os.path.exists(path):
        time.sleep(0.05)


def toks(seq):
    for t in seq:
        thought(t)


def run(p):
    p = p.lower()
    if "tokemph" in p:
        toks(["I", " should", " check", " the", " synthetic", " ", "**", "loader", ".toml", "**",
              " file", " before", " changing", " the", " default", "."])
        pause("tokemph")
        answer("Tokemph scenario answered.")
    elif "twohead" in p:
        toks(["**", "Reading", " the", " loader", "**", "\n\n", "It", " merges", " sources", ".", "\n\n"])
        toks(["**", "Planning", " the", " alias", " change", "**", "\n\n", "One", " place", "."])
        pause("twohead")
        answer("Twohead scenario answered.")
    elif "gemini" in p:
        thought("**Inspecting the synthetic loader**\nI am reading how it merges sources.")
        thought("**Planning the alias change**\nFlip the default in one place.")
        pause("gemini")
        answer("Gemini scenario answered.")
    elif "splitnot" in p:
        toks(["The loader is ", "**not**", " safe to change yet."])
        answer("Splitnot scenario answered.")
    elif "mixed" in p:
        thought("Listing the synthetic fixture directory first.")
        tool("Read fixtures/profile.toml")
        thought("The profile names look stable; next the alias table.")
        answer("Intermediate note. ")
        thought("**Verifying the alias table**\n\nTwo entries remain.")
        answer("Mixed scenario answered.")
    elif "endturn" in p:
        answer("Endturn scenario answered.")
        thought("Nothing left to change after the synthetic edit.")
    elif "startturn" in p:
        thought("Starting the next synthetic turn fresh.")
        answer("Startturn scenario answered.")
    else:
        answer("Ordinary synthetic reply.")


def main():
    global session_id
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        msg = json.loads(line)
        method, mid = msg.get("method"), msg.get("id")
        if method == "initialize":
            send({"jsonrpc": "2.0", "id": mid, "result": {
                "protocolVersion": 1, "agentCapabilities": {"loadSession": LOAD},
                "authMethods": [], "agentInfo": {"name": "synth-agent", "version": "0.0.1"}}})
        elif method == "session/new":
            session_id = "synth-" + uuid.uuid4().hex[:8]
            send({"jsonrpc": "2.0", "id": mid, "result": {"sessionId": session_id}})
        elif method == "session/load":
            session_id = msg["params"]["sessionId"]
            path = os.path.join(LOG_DIR, session_id + ".jsonl")
            if os.path.exists(path):
                for l in open(path):
                    update(json.loads(l), record=False)
            send({"jsonrpc": "2.0", "id": mid, "result": {}})
        elif method == "session/prompt":
            text = " ".join(b.get("text", "") for b in msg["params"].get("prompt", []) if b.get("type") == "text")
            last = text.strip().splitlines()[-1] if text.strip() else ""
            with open(os.path.join(LOG_DIR, session_id + ".jsonl"), "a") as f:
                f.write(json.dumps({"sessionUpdate": "user_message_chunk", "content": {"type": "text", "text": last}}) + "\n")
            run(last)
            send({"jsonrpc": "2.0", "id": mid, "result": {"stopReason": "end_turn"}})
        elif mid is not None:
            send({"jsonrpc": "2.0", "id": mid, "result": None})


if __name__ == "__main__":
    main()
