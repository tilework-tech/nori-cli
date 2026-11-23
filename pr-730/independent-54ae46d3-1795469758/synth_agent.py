#!/usr/bin/env python3
"""Synthetic ACP agent for verifying thought-status behavior.

Each prompt selects a scenario by keyword. Between steps the agent blocks until
the next gate file ($SYNTH_GATE_DIR/<n>) exists, so a driver can capture each
frame deterministically. All text is synthetic.
"""
import json
import os
import sys
import time

GATE_DIR = os.environ.get("SYNTH_GATE_DIR", "/tmp/synth-gates")
os.makedirs(GATE_DIR, exist_ok=True)
gate_no = 0
session_id = "synth-session-1"
tool_no = 0


def send(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()


def update(u):
    send({"jsonrpc": "2.0", "method": "session/update",
          "params": {"sessionId": session_id, "update": u}})


def thought(text):
    update({"sessionUpdate": "agent_thought_chunk",
            "content": {"type": "text", "text": text}})


def answer(text):
    update({"sessionUpdate": "agent_message_chunk",
            "content": {"type": "text", "text": text}})


def tool(title, kind="read"):
    global tool_no
    tool_no += 1
    tid = f"synth-tool-{tool_no}"
    update({"sessionUpdate": "tool_call", "toolCallId": tid, "title": title,
            "kind": kind, "status": "in_progress"})
    time.sleep(0.2)
    update({"sessionUpdate": "tool_call_update", "toolCallId": tid,
            "status": "completed",
            "content": [{"type": "content", "content": {"type": "text", "text": "synthetic output"}}]})


def gate(label):
    global gate_no
    gate_no += 1
    path = os.path.join(GATE_DIR, str(gate_no))
    with open(os.path.join(GATE_DIR, "waiting"), "w") as f:
        f.write(f"{gate_no} {label}\n")
    while not os.path.exists(path):
        time.sleep(0.05)


def stream(fn, text, pieces=4):
    n = max(1, len(text) // pieces)
    for i in range(0, len(text), n):
        fn(text[i:i + n])
        time.sleep(0.05)


LONG = ("Comparing the synthetic loader defaults against the fixture profile so the "
        "migration keeps every alias, then checking that the retry budget still covers "
        "the slow network case before touching anything else in the module")


def run(prompt):
    p = prompt.lower()
    if "plain" in p:
        stream(thought, "Reading the synthetic loader before changing the default.\nThen the fixture tests.")
        gate("plain thought")
        stream(answer, "Plain scenario answered.")
    elif "bold" in p:
        thought("**Inspect")
        gate("half-streamed bold header")
        thought("ing fixture config**\n\nThe synthetic config has two profiles.\n\n")
        gate("first bold header")
        stream(thought, "**Planning the alias migration**\n\nKeep both names working.")
        gate("second bold header same block")
        stream(answer, "Bold scenario answered.")
    elif "multi" in p:
        stream(thought, "Listing the synthetic fixture directory first.")
        gate("thought A")
        tool("Read fixtures/profile.toml")
        gate("after tool, before thought B")
        stream(thought, "The profile names look stable; next the alias table.")
        gate("thought B")
        answer("Intermediate note from the agent. ")
        stream(thought, "**Verifying the alias table**\n\nTwo entries remain.")
        gate("thought C")
        stream(answer, "Multi scenario answered.")
    elif "long" in p:
        stream(thought, LONG)
        gate("long thought")
        stream(answer, "Long scenario answered.")
    elif "wide" in p:
        stream(thought, "合成設定ファイルを読み込んで既定値を比較し、移行で別名がすべて保持されることを確認しています")
        gate("wide thought")
        stream(answer, "Wide scenario answered.")
    elif "inline" in p:
        stream(thought, "I should open the synthetic **loader.toml** file before editing the default profile.")
        gate("inline bold")
        stream(answer, "Inline scenario answered.")
    else:
        gate("ordinary reply, no thoughts")
        stream(answer, "Ordinary synthetic reply.")


def main():
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        msg = json.loads(line)
        method = msg.get("method")
        mid = msg.get("id")
        if method == "initialize":
            send({"jsonrpc": "2.0", "id": mid, "result": {
                "protocolVersion": 1,
                "agentCapabilities": {"loadSession": False},
                "authMethods": [],
                "agentInfo": {"name": "synth-agent", "version": "0.0.1"}}})
        elif method == "session/new":
            send({"jsonrpc": "2.0", "id": mid, "result": {"sessionId": session_id}})
        elif method == "session/prompt":
            text = " ".join(b.get("text", "") for b in msg["params"].get("prompt", [])
                            if b.get("type") == "text")
            # Use only the last line so injected context does not pick a scenario.
            run(text.strip().splitlines()[-1] if text.strip() else "")
            send({"jsonrpc": "2.0", "id": mid, "result": {"stopReason": "end_turn"}})
        elif mid is not None:
            send({"jsonrpc": "2.0", "id": mid, "result": None})


if __name__ == "__main__":
    main()
