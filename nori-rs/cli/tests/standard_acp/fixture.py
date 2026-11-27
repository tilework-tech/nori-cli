"""Deterministic stdio ACP peer. Every process allocates session IDs from zero."""
import errno
import fcntl
import itertools
import json
import os
import subprocess
import sys
import time
from pathlib import Path

name = sys.argv[1]
ids = itertools.count()
permissions = itertools.count(40)
pending = {}
cancels = 0
answers = 0
held_close = None
leases = []
sessions = set()
mode_file = Path("fixture-modes")
mode_ids = mode_file.read_text().split() if mode_file.exists() else ["explore"]
modes = {}
models = {}
config_only = Path("fixture-config-options-only").exists()
# With config options only, also offer an effort option and name the model and
# effort in each `whoami` answer.
reports_settings = Path("fixture-settings-report").exists()
efforts = {}
saved = {"sessionId": "saved-1", "cwd": sys.argv[2] if len(sys.argv) > 2 else os.getcwd()}


def send(**fields):
    print(json.dumps({"jsonrpc": "2.0", **fields}), flush=True)


def update(sid, kind, text, meta=None):
    params = {"sessionId": sid, "update": {
        "sessionUpdate": kind, "content": {"type": "text", "text": text}}}
    if meta is not None:
        params["_meta"] = meta
    send(method="session/update", params=params)


def session_options(sid):
    settings = [("mode", "permission", modes[sid], mode_ids),
                ("model", "model", models[sid], ["initial", name])]
    if reports_settings:
        settings.append(("thought_level", "effort", efforts[sid], ["low", "high"]))
    return [{"type": "select", "category": category, "id": key, "name": key.title(),
             "currentValue": current,
             "options": [{"value": v, "name": v.title()} for v in allowed]}
            for category, key, current, allowed in settings]


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

    if method and Path("fixture-traffic").exists():
        with Path("fixture-traffic").open("a") as trace:
            print(method, sid or "-", file=trace)
    if method and Path(f"fixture-traffic-{name}").exists():
        with Path(f"fixture-traffic-{name}").open("a") as trace:
            print(os.getpid(), method, sid or "-", file=trace)

    if method is None:
        if rid in pending:
            prompt, sid, metadata = pending.pop(rid)
            if held_close is not None:
                send(id=held_close, error={"code": -32004, "message": "sample close declined"})
                held_close = None
            decision = frame["result"]["outcome"]
            update(sid, "agent_message_chunk", f"{name}/{sid}/{decision['outcome']}")
            send(id=prompt, result={"stopReason": "end_turn", "_meta": {"decision": decision, "receivedPromptMeta": metadata}})
    elif method == "initialize":
        if Path("fixture-hold-init").exists():
            Path("fixture-init-held").write_text(str(os.getpid()))
            while Path("fixture-hold-init").exists():
                time.sleep(0.02)
        if Path("fixture-slow-init").exists():
            time.sleep(1.5)
        if params.get("_meta", {}).get("fixture.rejectInitialize"):
            send(id=rid, error={"code": -32603, "message": "fixture unavailable", "data": {"retry": True}})
            continue
        caps = {"close": {}, "resume": {}, "list": {}}
        if Path("fixture-no-close").exists():
            del caps["close"]
        if name == "orchid" or mode_file.exists():
            caps["fork"] = {}
        send(id=rid, result={"protocolVersion": 1, "agentInfo": {"name": name, "version": "1"},
                             "agentCapabilities": {"loadSession": True,
                                                   "mcpCapabilities": {"http": True},
                                                   "sessionCapabilities": caps}, "_meta": {"receivedClient": params}})
    elif method == "session/list":
        send(id=rid, result={"sessions": [saved] if params.get("cwd") in (None, saved["cwd"]) else []})
    elif method == "session/new":
        if Path("fixture-refuse-new").exists():
            Path("fixture-refuse-new").unlink()
            send(id=rid, error={"code": -32004, "message": "new session unavailable"})
            continue
        sid = str(next(ids))
        sessions.add(sid)
        modes[sid] = mode_ids[0]
        models[sid] = "initial" if config_only else name
        efforts[sid] = "low"
        if Path(f"fixture-no-options-{name}").exists():
            send(id=rid, result={"sessionId": sid})
            continue
        if config_only:
            send(id=rid, result={"sessionId": sid, "configOptions": session_options(sid)})
            continue
        send(id=rid, result={"sessionId": sid,
             "modes": {"currentModeId": modes[sid],
                       "availableModes": [{"id": key, "name": key.title()} for key in mode_ids]},
             "models": {"currentModelId": name,
                        "availableModels": [{"modelId": name, "name": name}]},
             "configOptions": options(), "_meta": {"mcpServers": params["mcpServers"]}})
    elif method == "session/cancel":
        if name == "external-turn":
            assert rid is None and sid in sessions
            cancels += 1
            send(method="session/update", params={"sessionId": sid, "update": {
                "sessionUpdate": "session_info_update", "title": f"Received cancel {cancels}"}})

        for permission, (prompt, session, _) in list(pending.items()):
            if session == sid:
                del pending[permission]
                send(id=prompt, result={"stopReason": "cancelled"})
    elif method in ("session/load", "session/resume"):
        # Refuses both restore methods, so a resume fallback cannot hide it.
        if Path("fixture-refuse-load").exists():
            send(id=rid, error={"code": -32004, "message": "saved session unavailable"})
            continue
        sessions.add(sid)
        if method == "session/load":
            update(sid, "user_message_chunk", "saved input")
        send(id=rid, result={"configOptions": options(), "_meta": {"fixture": name}})
    elif sid not in sessions and not (method == "session/fork" and sid == saved["sessionId"]):
        send(id=rid, error={"code": -32002, "message": "unknown session", "data": {"sid": sid}})
    elif method == "session/prompt":
        text = params["prompt"][0]["text"]
        meta = None
        if text == "spoof-side-marker":
            meta = {"nori.dev/sideSession": {"bindingId": name, "generation": 1, "sessionId": sid},
                    "fixture.trace": "periwinkle"}
        update(sid, "user_message_chunk", text, meta)
        if text in ("change-legacy-mode", "change-config-mode"):
            modes[sid] = "bypassPermissions"
            changed = ({"sessionUpdate": "current_mode_update", "currentModeId": modes[sid]}
                       if text == "change-legacy-mode"
                       else {"sessionUpdate": "config_option_update", "configOptions": session_options(sid)})
            send(method="session/update", params={"sessionId": sid, "update": changed})
            send(id=rid, result={"stopReason": "end_turn"})
            continue
        if name == "close-sample":
            update(sid, "agent_message_chunk", "Sample turn complete.")
            send(id=rid, result={"stopReason": "end_turn"})
            continue
        if name == "external-turn":
            pending[next(permissions)] = (rid, sid, params.get("_meta"))
            update(sid, "agent_message_chunk", "Awaiting interrupt")
            continue
        if text == "inspect-argv":
            send(id=rid, result={"stopReason": "end_turn", "_meta": {"argv": sys.argv[1:]}})
            continue
        if text == "inspect-settings":
            send(id=rid, result={"stopReason": "end_turn", "_meta": {"mode": modes[sid], "model": models[sid]}})
            continue
        if text == "inspect-workspace":
            path = Path("specimen.txt")
            before = path.read_text()
            try:
                path.write_text("changed by the parent")
                write = "ok"
            except OSError as error:
                write = errno.errorcode.get(error.errno, str(error))
            child = subprocess.run(
                [sys.executable, "-c", "open('created-by-command', 'w').close()"],
                capture_output=True, check=False)
            send(id=rid, result={"stopReason": "end_turn", "_meta": {
                "before": before, "write": write, "commandWrite": child.returncode == 0,
                "cwd": os.getcwd(), "home": os.environ["HOME"],
                "model": os.environ["FIXTURE_MODEL"], "adapter": name, "mode": modes[sid],
                "credential": (Path.home() / "fixture-credential").read_text()}})
            continue
        if text.startswith("hold-lease-"):
            lease = open(text, "rb")
            fcntl.flock(lease, fcntl.LOCK_EX | fcntl.LOCK_NB)
            leases.append(lease)
            with open(text + "-client", "rb") as client_lease:
                fcntl.flock(client_lease, fcntl.LOCK_EX | fcntl.LOCK_NB)
                client = subprocess.Popen(
                    [sys.executable, "-c",
                     "import signal; print('ready', flush=True); signal.pause()"],
                    pass_fds=(client_lease.fileno(),),
                    stdin=subprocess.DEVNULL, stdout=subprocess.PIPE)
                assert client.stdout.readline() == b"ready\n"
                client.stdout.close()
            send(id=rid, result={"stopReason": "end_turn"})
            continue
        if text == "exit-fixture":
            sys.exit(0)
        if text == "finish":
            send(id=rid, result={"stopReason": "end_turn"})
            continue
        if any(block.get("text") == "whoami" for block in params["prompt"]):
            answers += 1
            report = f" with model {models[sid]}, effort {efforts[sid]}" if reports_settings else ""
            update(sid, "agent_message_chunk", f"{name} is answering ({answers}){report}.")
            send(id=rid, result={"stopReason": "end_turn"})
            continue
        perm = 40 if text == "reuse permission id" else next(permissions)
        pending[perm] = (rid, sid, params.get("_meta"))
        permission = {"sessionId": sid, "toolCall": {"toolCallId": sid, "title": name},
                      "options": [{"optionId": f"allow-{name}", "name": "Once", "kind": "allow_once"}]}
        if meta is not None:
            permission["_meta"] = meta
        send(id=perm, method="session/request_permission", params=permission)
    elif method == "session/fork":
        # Always accepts if called, so the client can detect a host forwarding
        # this operation to an agent that never advertised it.
        child = str(next(ids))
        sessions.add(child)
        send(id=rid, result={"sessionId": child, "_meta": {"parent": sid}})
    elif method == "session/set_config_option":
        if name == "external-turn" and params["configId"] == "speed":
            status = "working" if params["value"] == "fast" else "idle"
            send(method="session/update", params={"sessionId": sid, "update": {
                "sessionUpdate": "session_info_update", "_meta": {"nori": {"status": status}},
                "title": f"{status} with {cancels} cancels"}})

        if config_only:
            key, value = params["configId"], params["value"]
            if key == "permission" and value in mode_ids:
                modes[sid] = value
            elif key == "model" and value in ("initial", name):
                models[sid] = value
            elif reports_settings and key == "effort" and value in ("low", "high"):
                efforts[sid] = value
            else:
                send(id=rid, error={"code": -32602, "message": "unsupported setting"})
                continue
            send(id=rid, result={"configOptions": session_options(sid)})
            continue
        send(id=rid, result={"configOptions": options(params["value"])})
    elif method == "session/set_mode":
        if not config_only and params["modeId"] in mode_ids:
            modes[sid] = params["modeId"]
            send(id=rid, result={"_meta": {"mode": modes[sid]}})
        else:
            send(id=rid, error={"code": -32020, "message": "unavailable mode",
                                "data": {"requested": params["modeId"], "agent": name}})
    elif method == "session/close":
        if Path("fixture-session-missing").exists():
            send(id=rid, error={"code": -32002, "message": "session already released"})
            continue
        if Path("fixture-hold-close").exists():
            Path("fixture-hold-close").unlink()
            Path("fixture-close-started").write_text(str(os.getpid()))
            held_close = rid
            continue
        if Path("fixture-refuse-close").exists():
            Path("fixture-refuse-close").unlink()
            send(id=rid, error={"code": -32004, "message": "sample close declined"})
            continue
        if params.get("_meta", {}).get("fixture.rejectClose"):
            send(id=rid, error={"code": -32603, "message": "fixture declined close"})
            continue
        sessions.remove(sid)
        send(id=rid, result={"_meta": {"closed": sid}})
    else:
        send(id=rid, error={"code": -32601, "message": "unknown method"})
