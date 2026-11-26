"""Logging WebSocket relay: client -> 127.0.0.1:LISTEN -> UPSTREAM. Logs find_paths requests and replies."""
import asyncio, json, sys, time
import websockets
LISTEN, UPSTREAM, LOG = int(sys.argv[1]), sys.argv[2], open(sys.argv[3], "a", buffering=1)
async def handle(client):
    hdrs = {k: v for k, v in client.request.headers.raw_items() if k.lower() == "authorization"}
    async with websockets.connect(UPSTREAM, additional_headers=hdrs, max_size=None) as up:
        pending = {}
        async def pump(src, dst, way):
            async for msg in src:
                try:
                    d = json.loads(msg)
                    if d.get("method") == "_nori/session/find_paths":
                        pending[d.get("id")] = d["params"]
                        LOG.write(json.dumps({"t": time.time(), "way": way, "request": d["params"]}) + "\n")
                    elif "id" in d and d.get("id") in pending and ("result" in d or "error" in d):
                        LOG.write(json.dumps({"t": time.time(), "way": way, "for": pending.pop(d["id"]), "reply": d.get("result", d.get("error"))}) + "\n")
                except Exception:
                    pass
                await dst.send(msg)
        await asyncio.gather(pump(client, up, "c2h"), pump(up, client, "h2c"))
async def main():
    async with websockets.serve(handle, "127.0.0.1", LISTEN, max_size=None):
        await asyncio.Future()
asyncio.run(main())
