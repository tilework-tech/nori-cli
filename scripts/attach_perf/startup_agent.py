"""Local ACP server for timing startup. All frames contain synthetic values."""
import json
from pathlib import Path
import sys
import time


def run(trace, rows, turns, live_chunks):
    def stamp(event):
        trace.write(f"{time.monotonic_ns()} {event}\n")
        trace.flush()

    def send(**frame):
        print(json.dumps({"jsonrpc": "2.0", **frame}), flush=True)

    active_session = None
    stamp("agent_enter")
    for line in sys.stdin:
        frame = json.loads(line)
        method = frame.get("method")
        if method is None or "id" not in frame:
            continue
        stamp(method + "_rx")
        if method == "initialize":
            result = {"protocolVersion": 1, "agentInfo": {"name": "timing-probe", "version": "1"},
                      "agentCapabilities": {"loadSession": True, "sessionCapabilities": {"list": {}}}}
        elif method in ("session/new", "session/load"):
            result = {"configOptions": [{"type": "select", "id": "model", "category": "model",
                                         "name": "Model", "currentValue": "ready",
                                         "options": [{"value": "ready", "name": "CobaltReady"}]}]}
            if method == "session/load":
                active_session = frame["params"]["sessionId"]
                if not active_session.startswith("saved-"):
                    raise ValueError(f"unexpected restore target {active_session}")
                for turn in range(turns):
                    for kind, value in (("user_message_chunk", f"Check synthetic row {turn}"),
                                        ("agent_message_chunk", f"**PolishedRow{turn:06d}** verified.\n")):
                        send(method="session/update", params={"sessionId": frame["params"]["sessionId"], "update": {
                            "sessionUpdate": kind, "content": {"type": "text", "text": value}}})
                send(method="session/update", params={"sessionId": frame["params"]["sessionId"], "update": {
                    "sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "CobaltTail"}}})
            if method == "session/new":
                active_session = "synthetic-startup"
                result["sessionId"] = active_session
        elif method == "session/list":
            result = {"sessions": [{"sessionId": f"saved-{i:04d}", "cwd": str(Path.cwd()),
                                    "title": f"CobaltArchive{i:04d}"} for i in range(rows)]}
        elif method == "session/prompt":
            if frame["params"]["sessionId"] != active_session or (rows and not active_session.startswith("saved-")):
                raise ValueError("prompt targeted a different session")
            if not any(part.get("text", "").endswith("§") for part in frame["params"]["prompt"]):
                raise ValueError("rendered draft did not reach the synthetic agent")
            def reply(text):
                send(method="session/update", params={"sessionId": active_session, "update": {
                    "sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": text}}})
            if live_chunks:
                stamp("live_start")
                reply("CobaltLiveStart\n\n")
                for index in range(live_chunks):
                    reply(f"- **Wave {index}** sampled at the synthetic station.\n")
                    time.sleep(0.01)
                stamp("live_complete")
            reply("\nCobaltReceived")
            result = {"stopReason": "end_turn"}
        else:
            send(id=frame["id"], error={"code": -32601, "message": "Unsupported fixture method"})
            continue
        stamp(method + "_reply")
        send(id=frame["id"], result=result)


if __name__ == "__main__":
    with Path(sys.argv[1]).open("a", encoding="utf-8") as ledger:
        run(ledger, *map(int, sys.argv[2:]))
