#!/usr/bin/env python3
"""Test-only TCP relay controlled by a text file: forward/drop/blackhole/stop."""
import argparse
import json
import select
import socket
from pathlib import Path


def serve(target_host, target_port, listen_port, control, ready):
    listener = socket.socket()
    listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    listener.bind(("127.0.0.1", listen_port))
    listener.listen()
    listener.setblocking(False)
    connections = []
    mode = "forward"
    count = 0
    delivered = 0

    def close(pair):
        for sock in pair[:2]:
            sock.close()

    def status():
        temporary = ready.with_suffix(ready.suffix + ".tmp")
        temporary.write_text(json.dumps({"port": listener.getsockname()[1],
            "endpoint": f"ws://127.0.0.1:{listener.getsockname()[1]}/sessions",
            "mode": mode, "connections": len(connections),
            "accepted": count, "forwarded_bytes": delivered}) + "\n")
        temporary.replace(ready)

    status()
    try:
        while True:
            requested = (control.read_text().strip() or mode) if control.exists() else "forward"
            if requested not in {"forward", "drop", "blackhole", "stop"}:
                raise ValueError(f"Invalid proxy mode: {requested!r}")
            if requested != mode:
                mode = requested
                if mode in {"drop", "stop"}:
                    for pair in connections:
                        close(pair)
                    connections.clear()
                status()
            if mode == "stop":
                return
            readable, _, _ = select.select([listener], [], [], 0.02)
            if readable:
                client, _ = listener.accept()
                if mode == "drop":
                    client.close()
                else:
                    try:
                        upstream = socket.create_connection((target_host, target_port), timeout=1)
                    except OSError:
                        client.close()
                    else:
                        client.setblocking(False)
                        upstream.setblocking(False)
                        connections.append([client, upstream, bytearray(), bytearray()])
                        count += 1
                        status()
            if mode != "forward":
                continue
            for pair in connections[:]:
                failed = False
                for source, destination, pending in [(pair[0], pair[1], pair[2]),
                                                       (pair[1], pair[0], pair[3])]:
                    try:
                        if len(pending) < 65536:
                            readable, _, _ = select.select([source], [], [], 0)
                            if readable:
                                data = source.recv(8192)
                                if not data:
                                    failed = True
                                    break
                                pending.extend(data)
                        if pending:
                            _, writable, _ = select.select([], [destination], [], 0)
                            if writable:
                                sent = destination.send(pending)
                                if not sent:
                                    failed = True
                                    break
                                del pending[:sent]
                                delivered += sent
                    except OSError:
                        failed = True
                        break
                if failed:
                    close(pair)
                    connections.remove(pair)
                    status()
    finally:
        for pair in connections:
            close(pair)
        listener.close()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target-host", default="127.0.0.1")
    parser.add_argument("--target-port", type=int, required=True)
    parser.add_argument("--listen-port", type=int, default=0)
    parser.add_argument("--control", type=Path, required=True)
    parser.add_argument("--ready", type=Path, required=True)
    args = parser.parse_args()
    serve(args.target_host, args.target_port, args.listen_port, args.control, args.ready)
