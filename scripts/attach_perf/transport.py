"""Optional wire diagnostic for HostFixture. Use direct attach for primary timings."""

import copy
import ipaddress
import json
import re
import select
import socket
import threading
import time
from urllib.parse import urlsplit

from agent import FENCE, FIRST, LAST, LIVE


class FrameReader:
    """Read text messages from the fixture's uncompressed WebSocket stream."""

    def __init__(self):
        self.buffer = bytearray()
        self.handshake = True
        self.fragments = bytearray()

    def messages(self, data):
        self.buffer.extend(data)
        if self.handshake:
            end = self.buffer.find(b"\r\n\r\n")
            if end < 0:
                return
            del self.buffer[:end + 4]
            self.handshake = False

        buf = self.buffer
        while len(buf) >= 2:
            flags, length = buf[:2]
            masked, size = bool(length & 128), length & 127
            extra = {126: 2, 127: 8}.get(size, 0)
            header = 2 + extra + (4 if masked else 0)
            if len(buf) < header:
                return
            if extra:
                size = int.from_bytes(buf[2:2 + extra], "big")
            if len(buf) < header + size:
                return
            mask = buf[2 + extra:header]
            payload = buf[header:header + size]
            del buf[:header + size]
            if flags & 112:
                raise ValueError("Fixture negotiated an unsupported WebSocket extension")
            if (flags & 15) not in (0, 1):
                continue  # Ping, pong, close or binary.
            if masked:
                payload = bytes(value ^ mask[i % 4] for i, value in enumerate(payload))
            self.fragments.extend(payload)
            if flags & 128:
                yield bytes(self.fragments)
                self.fragments.clear()


class Trace:
    """Retain only counts, timestamps, and known synthetic beacons."""

    def __init__(self, session_id):
        self.session_id = session_id
        self.lock = threading.Lock()
        self.data = dict(subscriptions=[], event_count=0, latest_seq=0, gaps=0,
                         first_event_ns=None, last_event_ns=None, replay_finished_ns=None,
                         history_watermark=None, history_through_ns=None, last_listed_seq=None,
                         marker_seq={}, marker_ns={})
        self.markers = re.compile(b"|".join([re.escape(m.encode()) for m in (FIRST, LAST, FENCE)]
                                            + [re.escape(LIVE.encode()) + rb"[0-9]{4,}"]))

    def observe(self, outbound, payload, now):
        frame = json.loads(payload)
        method = frame.get("method")
        params = frame.get("params", {})
        with self.lock:
            d = self.data
            if outbound and method == "_nori/session/subscribe" and params.get("session_id") == self.session_id:
                cursor = params["cursors"][self.session_id]
                d["subscriptions"].append(dict(at_ns=now, cursor=cursor, delivered_seq=d["latest_seq"]))
                if len(d["subscriptions"]) == 1:
                    d["history_watermark"] = d["last_listed_seq"]
            elif not outbound and isinstance(frame.get("result"), dict) and "sessions" in frame["result"]:
                for row in frame["result"]["sessions"]:
                    if row.get("session_id") == self.session_id:
                        d["last_listed_seq"] = row.get("latest_seq")
            elif not outbound and method == "_nori/session/event" and params.get("conversation_id") == self.session_id:
                seq = params["seq"]
                if seq != d["latest_seq"] + 1:
                    d["gaps"] += 1
                d["latest_seq"] = seq
                d["event_count"] += 1
                d["first_event_ns"] = d["first_event_ns"] or now
                d["last_event_ns"] = now
                if seq == d["history_watermark"]:
                    d["history_through_ns"] = now
                event = params["event"]
                if event.get("source") == "nori" and event["event"].get("event_type") == "replay_finished":
                    d["replay_finished_ns"] = now
                if event.get("source") == "acp":
                    for marker in self.markers.findall(payload):
                        name = marker.decode()
                        d["marker_seq"].setdefault(name, seq)
                        d["marker_ns"].setdefault(name, now)

    def snapshot(self):
        with self.lock:
            return copy.deepcopy(self.data)

    def verify_applied(self, live_marker=f"{LIVE}0001"):
        d = self.snapshot()
        if d["gaps"] or len(d["subscriptions"]) < 2 or live_marker not in d["marker_seq"]:
            raise AssertionError("Need contiguous replay, a live beacon and a subsequent subscription")
        last = d["subscriptions"][-1]
        required = max(d["history_watermark"] or 0, d["marker_seq"][live_marker], last["delivered_seq"])
        if last["cursor"] < required or last["at_ns"] < d["marker_ns"][live_marker]:
            raise AssertionError(f"Reselection cursor {last['cursor']} did not confirm sequence {required}")


class TransportProbe(Trace):
    """Observe the same real socket used by attach in an independent diagnostic run."""

    def __init__(self, home, session_id):
        super().__init__(session_id)
        state = json.loads((home / "host/state.json").read_text())
        self.upstream = urlsplit(state["endpoint"])
        if self.upstream.scheme != "ws" or not ipaddress.ip_address(self.upstream.hostname).is_loopback:
            raise ValueError("Wire diagnostic requires a loopback fixture endpoint")
        self.token_path = home / "host/token"
        self.stopped = threading.Event()
        self.error = None

    def __enter__(self):
        self.listener = socket.create_server(("127.0.0.1", 0))
        self.listener.settimeout(.1)
        port = self.listener.getsockname()[1]
        endpoint = f"ws://127.0.0.1:{port}{self.upstream.path}"
        self.attach_args = ["--host", endpoint, "--token-file", str(self.token_path)]
        self.thread = threading.Thread(target=self._run, daemon=True)
        self.thread.start()
        return self

    def _run(self):
        try:
            while not self.stopped.is_set():
                try:
                    client, _ = self.listener.accept()
                except TimeoutError:
                    continue
                with client, socket.create_connection((self.upstream.hostname, self.upstream.port), 3) as host:
                    for sock in (client, host):
                        sock.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
                        sock.settimeout(3)
                    streams = {client: (host, FrameReader()), host: (client, FrameReader())}
                    while not self.stopped.is_set():
                        ready, _, _ = select.select(list(streams), [], [], .1)
                        closed = False
                        for sock in ready:
                            block = sock.recv(65536)
                            if not block:
                                closed = True
                                break
                            now = time.monotonic_ns()
                            peer, reader = streams[sock]
                            peer.sendall(block)
                            for msg in reader.messages(block):
                                self.observe(sock is client, msg, now)
                        if closed:
                            break
        except (BrokenPipeError, ConnectionResetError):
            pass  # A PTY run may kill the client to finish a sample.
        except Exception as error:
            self.error = error

    def __exit__(self, exc_type, _exc, _tb):
        self.stopped.set()
        self.thread.join(timeout=4)
        self.listener.close()
        if exc_type is None:
            if self.error:
                raise RuntimeError("Wire diagnostic failed") from self.error
            if self.thread.is_alive():
                raise RuntimeError("Wire diagnostic did not stop")


def sample(binary, turns, wrapper):
    from tempfile import TemporaryDirectory
    from pathlib import Path

    from fixture import HostFixture
    from replay import Terminal

    def wait_wire(probe, predicate):
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            if probe.error:
                raise RuntimeError("Wire diagnostic failed") from probe.error
            if predicate(probe.snapshot()):
                return
            time.sleep(.001)
        raise TimeoutError("Timed out observing the expected wire message")

    with HostFixture(binary, turns=turns) as fix, TransportProbe(fix.home, fix.session_id) as probe:
        with TemporaryDirectory(prefix="nori-wire-terminal-") as tmp:
            term = Terminal(Path(tmp), wrapper, fix, [str(binary), "attach", *probe.attach_args])
            try:
                term.launch()
                term.wait(lambda screen: "Sessions 1" in screen and "enter" in screen.lower())
                selection = time.monotonic_ns()
                term.send("Enter")
                wait_wire(probe, lambda row: bool(row["subscriptions"]))
                live_request = time.monotonic_ns()
                marker = fix.live()
                term.wait(lambda screen: marker in screen and "ready" in screen.splitlines()[1])
                live_observed = time.monotonic_ns()
                term.send("Left")
                term.wait(lambda screen: "Sessions 1" in screen)
                term.send("Enter")
                wait_wire(probe, lambda row: len(row["subscriptions"]) == 2)
                probe.verify_applied(marker)
                row = probe.snapshot()
            finally:
                term.close()
        if row["history_through_ns"] is None:
            raise AssertionError("First selection lacked a host journal watermark")
        if not 0 < row["marker_seq"][LAST] < row["marker_seq"][FENCE] <= row["history_watermark"] < row["marker_seq"][marker]:
            raise AssertionError("Reply, final tool, saved watermark and live beacon must be ordered")

    def ms(stamp):
        return round((stamp - selection) / 1e6, 3) if stamp is not None else None

    return {
        "turns": turns, "acp_updates": fix.counts["acp_updates"],
        "events": row["event_count"], "out_of_sequence": row["gaps"],
        "saved_journal_watermark": row["history_watermark"], "marker_seq": row["marker_seq"],
        "selection_to_subscribe_ms": ms(row["subscriptions"][0]["at_ns"]),
        "selection_to_first_wire_event_ms": ms(row["first_event_ns"]),
        "selection_to_saved_watermark_on_wire_ms": ms(row["history_through_ns"]),
        "selection_to_native_replay_finished_on_wire_ms": ms(row["replay_finished_ns"]),
        "selection_to_live_request_ms": ms(live_request),
        "selection_to_live_on_wire_ms": ms(row["marker_ns"][marker]),
        "selection_to_live_visible_and_ready_ms": ms(live_observed),
        "selection_to_reselect_request_ms": ms(row["subscriptions"][1]["at_ns"]),
        "initial_cursor": row["subscriptions"][0]["cursor"],
        "reselect_cursor": row["subscriptions"][1]["cursor"],
        "delivered_at_reselect": row["subscriptions"][1]["delivered_seq"],
    }


def main():
    import argparse
    import hashlib
    import os
    from pathlib import Path

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--build", nargs=2, action="append", required=True, metavar=("LABEL", "BINARY"))
    parser.add_argument("--turns", type=int, default=32)
    parser.add_argument("--tmux-wrapper", default=os.environ.get("NORI_BENCH_TMUX"))
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    result = {"method": "Loopback wire diagnostic in a 120x40 tmux terminal. All times start immediately "
                        "before Enter on the selected picker row. Wire times mark receipt of a websocket "
                        "read by the relay. Reselect cursor confirms contiguous application through that "
                        "sequence by the UI. Native replay_finished can be absent for journal catch-up.",
              "builds": {}}
    for label, path in args.build:
        binary = Path(path).resolve(strict=True)
        if label in result["builds"]:
            parser.error("Build labels must be unique")
        with binary.open("rb") as source:
            digest = hashlib.file_digest(source, "sha256").hexdigest()
        result["builds"][label] = {"binary_sha256": digest, **sample(binary, args.turns, args.tmux_wrapper)}
    output = json.dumps(result, indent=2) + "\n"
    if args.output:
        args.output.write_text(output)
    print(output, end="")


if __name__ == "__main__":
    main()
