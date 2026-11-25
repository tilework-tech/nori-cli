"""Time actual draft handling and the first frame carrying the agent's activated model.

Extract the startup benchmark from PR 764 first (see --help). Both processes use
the operating system's monotonic clock. Agent milestones record immediately before
flushing each reply. PTY milestones record arrival of the bytes in the parent.
"""
import argparse
from datetime import datetime
import errno
import fcntl
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import platform
import re
import pty
import select
import signal
import statistics
import struct
import subprocess
import sys
import tempfile
import termios
import time

DRAFT = "§".encode()
PROMPT = "›".encode()


def benchmark_module(path):
    spec = importlib.util.spec_from_file_location("startup764", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def observe(binary, cwd, env, bench, turns, live):
    master, slave = pty.openpty()
    process = None
    events = {}
    stream = bytearray()
    try:
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 120, 0, 0))
        modes = termios.tcgetattr(slave)
        modes[3] &= ~termios.ECHO
        termios.tcsetattr(slave, termios.TCSANOW, modes)
        wall_ns = time.time_ns()
        events["launch"] = time.monotonic_ns()
        process = subprocess.Popen(
            [binary, "--skip-trust-directory"], stdin=slave, stdout=slave, stderr=slave,
            cwd=cwd, env=env, preexec_fn=bench.take_controlling_terminal,
        )
        deadline = time.monotonic() + 15
        after_input = None
        picker_offset = None
        restore_offset = None
        painted = []
        seen = set()
        end_event = "after_stream_handled" if live else "delivered_on_screen"
        while end_event not in events:
            if process.poll() is not None or time.monotonic() >= deadline:
                raise RuntimeError(f"Missing milestone after {list(events)}: {stream[-3000:]!r}")
            if not select.select([master], [], [], 0.2)[0]:
                continue
            previous = len(stream)
            try:
                data = os.read(master, 65536)
            except OSError as error:
                if error.errno != errno.EIO:
                    raise
                data = b""
            if not data:
                raise RuntimeError("Terminal closed before completion")
            now = time.monotonic_ns()
            stream.extend(data)
            window = stream[max(0, previous - 24):]
            for query in bench.QUERY.finditer(stream, max(0, previous - 15)):
                if query.end() > previous:
                    os.write(master, bench.REPLIES[query.group()])
            if turns:
                if picker_offset is None and b"CobaltReady" in window:
                    picker_offset = len(stream)
                    events["open_picker"] = time.monotonic_ns()
                    os.write(master, b"/resume")
                elif (picker_offset is not None and "command" not in events
                      and b"/resume" in stream[picker_offset:]):
                    events["command"] = time.monotonic_ns()
                    os.write(master, b"\r")
                if restore_offset is None and picker_offset is not None and b"CobaltArchive" in stream[picker_offset:]:
                    events["listed"] = now
                    restore_offset = len(stream)
                    events["select"] = time.monotonic_ns()
                    os.write(master, b"\x1b[B\r")
                if restore_offset is not None:
                    # Look behind the read boundary for split tokens, never rescan history.
                    restored = stream[max(restore_offset, previous - 24):]
                    for match in re.finditer(rb"PolishedRow(\d{6})", restored):
                        row = int(match[1])
                        if row not in seen:
                            seen.add(row)
                            painted.append(row)
                            if len(painted) == turns:
                                events["all_replies_painted"] = now
                    if "first_content" not in events and (b"PolishedRow" in restored or b"CobaltTail" in restored):
                        events["first_content"] = now
                    if b"CobaltTail" in restored and after_input is None:
                        events["latest_reply"] = now
                        after_input = len(stream)
                        os.write(master, DRAFT)
            elif after_input is None and PROMPT in window:
                events["first_composer"] = now
                after_input = len(stream)
                os.write(master, DRAFT)
            if after_input is not None and "first_handled_input" not in events and DRAFT in stream[after_input:]:
                events["first_handled_input"] = now
            if "ui_ready" not in events and b"CobaltReady" in window:
                events["ui_ready"] = now
            if "submit" not in events and "ui_ready" in events and "first_handled_input" in events:
                events["submit"] = time.monotonic_ns()
                os.write(master, b"\r")
            if live and "live_first_frame" not in events and b"CobaltLiveStart" in window:
                events["live_first_frame"] = now
                events["live_type"] = time.monotonic_ns()
                os.write(master, "β".encode())
            if live and "live_handled" not in events and "β".encode() in window:
                events["live_handled"] = now
            if "delivered_on_screen" not in events and b"CobaltReceived" in window:
                events["delivered_on_screen"] = now
                if live:
                    events["after_stream_type"] = time.monotonic_ns()
                    os.write(master, "ζ".encode())
            if live and "ζ".encode() in window:
                events["after_stream_handled"] = now
        # Let the app stop its owned host/agent and reap its children on ordinary exit.
        os.write(master, b"\x04")
        quit_deadline = time.monotonic() + 0.5
        while process.poll() is None and time.monotonic() < quit_deadline:
            if select.select([master], [], [], 0.02)[0]:
                os.read(master, 65536)
    finally:
        if process is not None and process.poll() is None:
            os.killpg(process.pid, signal.SIGTERM)
            try:
                process.wait(timeout=2)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait()
        os.close(master)
        os.close(slave)
    if turns and painted != list(range(turns)):
        raise RuntimeError(f"PTY did not show each of {turns} replies in order: {painted}")
    if env.get("RUST_LOG"):
        pattern = re.compile(r"^(\S+) .*host ACP request.*\bmethod=(\S+)")
        for logfile in sorted((Path(env["NORI_HOME"]) / "log").glob("nori-acp*")):
            for line in logfile.read_text().splitlines():
                if match := pattern.search(line):
                    stamp, method = match.groups()
                    dt = datetime.fromisoformat(stamp.replace("Z", "+00:00"))
                    # Align wall-clock host logs with the other processes' monotonic clock.
                    since_launch = round(dt.timestamp() * 1e9) - wall_ns
                    events.setdefault(f"first_host_{method}", events["launch"] + since_launch)
    return events


def sample(binary, bench, rows, turns, host_trace, live_chunks):
    with tempfile.TemporaryDirectory(prefix="nori-activation-") as directory:
        home = Path(directory).resolve()
        cwd = home / "project"
        cwd.mkdir()
        ledger = home / "agent.trace"
        agent = Path(__file__).with_name("startup_agent.py").resolve()
        (home / "config.toml").write_text(
            f'agent = "timing-probe"\n[projects.{json.dumps(str(cwd))}]\ntrust_level = "trusted"\n'
            '[[agents]]\nname = "Timing probe"\nslug = "timing-probe"\n[agents.distribution.local]\n'
            f'command = {json.dumps(sys.executable)}\nargs = {json.dumps(["-I", str(agent), str(ledger), str(rows), str(turns), str(live_chunks)])}\n'
        )
        env = {"PATH": os.environ.get("PATH", os.defpath), "TERM": "xterm-256color", "LANG": "C.UTF-8",
               "HOME": str(home), "NORI_HOME": str(home), "CODEX_HOME": str(home), "NORI_NO_ANALYTICS": "1"}
        if host_trace:
            env["RUST_LOG"] = "nori_host_runtime=debug"
        events = observe(binary, cwd, env, bench, turns if rows else 0, live_chunks > 0)
        for line in ledger.read_text().splitlines():
            ns, event = line.split()
            # Opening the picker can prepare another adapter; keep every timestamp.
            key = event
            count = 1
            while key in events:
                count += 1
                key = f"{event}_{count}"
            events[key] = int(ns)
        required = {"agent_enter", "initialize_rx", "initialize_reply", "session/new_rx",
                    "session/new_reply", "session/prompt_rx", "session/prompt_reply"}
        if rows:
            required.update({"session/load_rx", "session/load_reply", "first_content", "latest_reply"})
        if live_chunks:
            required.update({"live_start", "live_first_frame", "live_type", "live_handled", "live_complete",
                             "after_stream_type", "after_stream_handled"})
        if missing := required - events.keys():
            raise RuntimeError(f"Incomplete agent trace: {missing}")
        if live_chunks and events["live_handled"] >= events["live_complete"]:
            raise RuntimeError("The application handled the typing probe only after the fixture finished streaming")
        return {key: round((at - events["launch"]) / 1e6, 3) for key, at in events.items()}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--benchmark", type=Path, required=True, help="PR 764 scripts/host_startup_benchmark.py")
    parser.add_argument("--binary", type=Path, action="append", required=True)
    parser.add_argument("--samples", type=int, default=20)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--saved-rows", type=int, default=0, help="open /resume with this many provider sessions")
    parser.add_argument("--turns", type=int, default=4, help="turns in the selected provider session")
    parser.add_argument("--host-trace", action="store_true", help="also collect owned host ACP dispatch times")
    parser.add_argument("--live-chunks", type=int, default=0, help="stream this many Markdown chunks at 10 ms intervals")
    args = parser.parse_args()
    if args.samples < 1 or args.turns < 1 or args.saved_rows < 0 or args.live_chunks < 0:
        parser.error("samples and turns must be positive, saved rows must be nonnegative")
    if args.saved_rows and args.live_chunks:
        parser.error("run saved-session and live-stream scenarios separately")
    bench = benchmark_module(args.benchmark)
    binaries = [path.resolve() for path in args.binary]
    runs = [[] for _ in binaries]
    for binary in binaries:
        sample(binary, bench, args.saved_rows, args.turns, args.host_trace, args.live_chunks)
    for i in range(args.samples):
        for j in range(len(binaries)):
            slot = (i + j) % len(binaries)
            runs[slot].append(sample(binaries[slot], bench, args.saved_rows, args.turns, args.host_trace, args.live_chunks))
    metrics = [{key: statistics.median(row[key] for row in series)
                for key in series[0] if all(key in row for row in series)} for series in runs]
    identities = []
    for binary in binaries:
        sha = hashlib.sha256()
        with binary.open("rb") as source:
            for part in iter(lambda: source.read(1024 * 1024), b""):
                sha.update(part)
        identities.append({"version": subprocess.check_output([binary, "--version"], text=True).strip(),
                           "sha256": sha.hexdigest()})
    report = {"platform": platform.platform(), "samples_per_binary": args.samples, "saved_rows": args.saved_rows, "turns": args.turns, "live_chunks": args.live_chunks, "dimensions": [120, 40],
              "binaries": identities, "times_from_launch_ms": runs, "median_ms": metrics}
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(metrics, indent=2))


if __name__ == "__main__":
    main()
