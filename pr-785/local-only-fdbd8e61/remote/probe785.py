"""Ask a running nori host to run `!` commands as a local and a remote controller."""
import asyncio, json, os, sys
import websockets

home, folder = sys.argv[1], sys.argv[2]
hostdir = os.path.join(home, "host")
discovery = json.load(open(os.path.join(hostdir, "host.json")))
token = discovery["token"]
local_token = open(os.path.join(hostdir, "local-token")).read().strip()
GRAY, GREEN, RED, BOLD, END = "\x1b[90m", "\x1b[32m", "\x1b[31m", "\x1b[1m", "\x1b[0m"

async def client(local, observe=False):
    headers = {"Authorization": f"Bearer {token}"}
    if local:
        headers["x-nori-local-token"] = local_token
    ws = await websockets.connect(discovery["endpoint"], additional_headers=headers)
    ws.next_id = 1
    init = await call(ws, "initialize", {"protocolVersion": 1, "observe": observe})
    return ws, init

async def call(ws, method, params):
    ws.next_id += 1
    rid = ws.next_id
    await ws.send(json.dumps({"jsonrpc": "2.0", "id": rid, "method": method, "params": params}))
    while True:
        frame = json.loads(await ws.recv())
        if frame.get("id") == rid and "method" not in frame:
            return frame

async def shell_output(ws, session):
    out = ""
    while True:
        frame = json.loads(await ws.recv())
        ev = frame.get("params", {}).get("event", {}).get("event", {})
        if ev.get("event_type") != "user_shell":
            continue
        e = ev["event"]
        if e["state"] == "started":
            print(f"  started   {e['command']!r} in {e['cwd']}")
        if e["state"] == "output":
            out += bytes(e["chunk"]).decode()
        if e["state"] == "finished":
            for line in out.splitlines():
                print(f"  output    {line}")
            print(f"  finished  exit {e['exit_code']}")
            return

def show(label, reply):
    if "error" in reply:
        e = reply["error"]
        print(f"{label}: {RED}error {e['code']}{END} {GRAY}{e['message']}{END}")
    else:
        print(f"{label}: {GREEN}started{END}")

async def main():
    remote, rinit = await client(local=False)
    local, linit = await client(local=True)
    watcher, _ = await client(local=True, observe=True)
    print(f"{BOLD}initialize -> capabilities.nori_sessions.shell{END}")
    print(f"  remote controller: {rinit['result']['capabilities']['nori_sessions']['shell']}")
    print(f"  local controller:  {linit['result']['capabilities']['nori_sessions']['shell']}")
    # The first client controls; the session is created by it.
    new = await call(remote, "session/new", {"cwd": folder, "mcpServers": []})
    sid = new["result"]["sessionId"]
    await call(watcher, "_nori/session/subscribe", {"session_id": sid})
    print(f"\n{BOLD}session folder{END} {folder}")
    print(f"\n{BOLD}remote controller (bearer token only) runs !touch marker{END}")
    show("  reply", await call(remote, "_nori/session/shell", {"sessionId": sid, "command": "touch marker"}))
    print(f"\n{BOLD}local controller (bearer + local token) takes control, runs !pwd; ls{END}")
    await call(local, "_nori/control/take", {})
    show("  reply", await call(local, "_nori/session/shell", {"sessionId": sid, "command": "pwd; ls"}))
    print(f"{GRAY}  as the watcher saw it:{END}")
    await shell_output(watcher, sid)
    print(f"\n{BOLD}watcher holding the local token runs !pwd{END}")
    show("  reply", await call(watcher, "_nori/session/shell", {"sessionId": sid, "command": "pwd"}))
    print(f"\n{BOLD}marker exists after the remote request?{END} {os.path.exists(os.path.join(folder, 'marker'))}")

asyncio.run(main())
