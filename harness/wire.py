"""Standard-library WebSocket JSON-RPC client for observing a nori host.

Used as an independent third client: it lists sessions, reads modes and
folders, and sends requests a TUI would send, to prove the host itself
enforces roles rather than only the screen.
"""
import base64
import hashlib
import json
import os
import socket
import struct
import time
from pathlib import Path
from urllib.parse import urlsplit

GUID = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"


def host_endpoint(home: Path, timeout=15):
    deadline = time.time() + timeout
    while time.time() < deadline:
        state, token = home / "host" / "state.json", home / "host" / "token"
        if state.exists() and token.exists():
            endpoint = json.loads(state.read_text()).get("endpoint")
            if endpoint:
                return endpoint, token.read_text().strip()
        time.sleep(0.2)
    raise TimeoutError(f"no host state under {home}")


class Wire:
    def __init__(self, endpoint, token, observe=True, log=None):
        url = urlsplit(endpoint)
        self.sock = socket.create_connection((url.hostname, url.port), timeout=10)
        key = base64.b64encode(os.urandom(16)).decode()
        request = (f"GET {url.path or '/'} HTTP/1.1\r\nHost: {url.netloc}\r\nUpgrade: websocket\r\n"
                   f"Connection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n"
                   f"Authorization: Bearer {token}\r\n\r\n")
        self.sock.sendall(request.encode())
        head = b""
        while b"\r\n\r\n" not in head:
            head += self.sock.recv(1)
        if b" 101 " not in head.split(b"\r\n")[0]:
            raise RuntimeError(head.decode(errors="replace"))
        expected = base64.b64encode(hashlib.sha1((key + GUID).encode()).digest())
        if expected not in head:
            raise RuntimeError("bad websocket accept")
        self.next_id, self.notes, self.log = 1, [], log
        self.init = self.call("initialize", {"observe": True} if observe else {})

    def _exact(self, n):
        data = b""
        while len(data) < n:
            part = self.sock.recv(n - len(data))
            if not part:
                raise EOFError("host closed the connection")
            data += part
        return data

    def _send(self, payload: bytes, opcode=1):
        mask = os.urandom(4)
        n = len(payload)
        head = bytes([0x80 | opcode])
        if n < 126:
            head += bytes([0x80 | n])
        elif n < 65536:
            head += bytes([0x80 | 126]) + struct.pack(">H", n)
        else:
            head += bytes([0x80 | 127]) + struct.pack(">Q", n)
        self.sock.sendall(head + mask + bytes(b ^ mask[i % 4] for i, b in enumerate(payload)))

    def _recv(self, timeout):
        self.sock.settimeout(timeout)
        message = b""
        while True:
            b1, b2 = self._exact(2)
            n = b2 & 0x7F
            if n == 126:
                n = struct.unpack(">H", self._exact(2))[0]
            elif n == 127:
                n = struct.unpack(">Q", self._exact(8))[0]
            data = self._exact(n)
            opcode = b1 & 0x0F
            if opcode == 9:
                self._send(data, 10)
                continue
            if opcode == 8:
                raise EOFError("close frame")
            message += data
            if b1 & 0x80:
                return json.loads(message)

    def _record(self, direction, frame):
        if self.log:
            with open(self.log, "a") as handle:
                handle.write(json.dumps({"t": time.time(), "dir": direction, "frame": frame}) + "\n")

    def call(self, method, params=None, timeout=15):
        rid = self.next_id
        self.next_id += 1
        frame = {"jsonrpc": "2.0", "id": rid, "method": method, "params": params or {}}
        self._record("out", frame)
        self._send(json.dumps(frame).encode())
        deadline = time.time() + timeout
        while time.time() < deadline:
            msg = self._recv(max(0.1, deadline - time.time()))
            self._record("in", msg)
            if msg.get("id") == rid and "method" not in msg:
                return msg
            self.notes.append(msg)
        raise TimeoutError(method)

    def drain(self, seconds=0.5):
        end = time.time() + seconds
        while time.time() < end:
            try:
                msg = self._recv(max(0.05, end - time.time()))
            except (socket.timeout, TimeoutError):
                break
            self._record("in", msg)
            self.notes.append(msg)
        return self.notes

    def sessions(self, cwd=None):
        params = {"cwd": cwd} if cwd else {}
        return self.call("session/list", params).get("result", {}).get("sessions", [])

    def close(self):
        try:
            self.sock.close()
        except OSError:
            pass
