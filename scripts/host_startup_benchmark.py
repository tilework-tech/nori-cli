#!/usr/bin/env python3
"""Compare release binaries on a single Linux or macOS host; see scripts/docs.md."""

import argparse
import errno
import fcntl
import json
import math
import os
from pathlib import Path
import platform
import pty
import random
import re
import select
import statistics
import struct
import subprocess
import sys
import tempfile
import termios
import time

TIMEOUT = 20
# Replies an xterm sends while crossterm initializes. Queries can span two reads.
REPLIES = {
    b"\x1b[6n": b"\x1b[1;1R",
    b"\x1b[c": b"\x1b[?1;2c",
    b"\x1b]10;?\x1b\\": b"\x1b]10;rgb:ffff/ffff/ffff\x1b\\",
    b"\x1b]11;?\x1b\\": b"\x1b]11;rgb:0000/0000/0000\x1b\\",
}
QUERY = re.compile(b"|".join(re.escape(query) for query in REPLIES))


def compare_samples(base, control, candidate):
    """Set each limit from two independent baseline series on the same machine."""
    regressions = []
    metrics = {}
    for key in base[0]:
        series = [sorted(row[key] for row in run) for run in (base, control, candidate)]
        rng = random.Random(0)
        baseline = series[0] + series[1]
        size = len(base)
        # Estimate ordinary variation of the percentile itself on this runner.
        resamples = [sorted(rng.choices(baseline, k=size)) for _ in range(2000)]
        for label, quantile in (
            ("p50", statistics.median),
            ("p95", lambda samples: samples[math.ceil(0.95 * len(samples)) - 1]),
        ):
            a, b, head = map(quantile, series)
            noise_ceiling = sorted(map(quantile, resamples))[1989]  # nearest-rank 99.5%
            limit = max(noise_ceiling, max(a, b) + abs(a - b))
            name = f"{key}.{label}"
            metrics[name] = {"base_ms": a, "repeat_ms": b, "head_ms": head, "limit_ms": limit}
            if head > limit:
                regressions.append(name)
    return {"metrics": metrics, "regressions": regressions}


def take_controlling_terminal():
    os.setsid()
    fcntl.ioctl(0, termios.TIOCSCTTY, 0)


def terminal_sample(binary, args, cwd, env, *, composer):
    """Time until the app draws its prompt, then renders a character typed into it."""
    master, slave = pty.openpty()
    try:
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 120, 0, 0))
        modes = termios.tcgetattr(slave)
        modes[3] &= ~termios.ECHO  # Only application-rendered input counts as interactive.
        termios.tcsetattr(slave, termios.TCSANOW, modes)
        start = time.monotonic()
        proc = subprocess.Popen(
            [binary, *args], stdin=slave, stdout=slave, stderr=slave, cwd=cwd, env=env,
            preexec_fn=take_controlling_terminal,
        )
        try:
            stream = bytearray()
            sent_input_at = None
            marker = "›".encode() if composer else b"Sessions 0"
            while True:
                left = start + TIMEOUT - time.monotonic()
                if left <= 0 or not select.select([master], [], [], left)[0]:
                    raise RuntimeError(f"{args} timed out waiting for {marker!r}: {stream[-500:]!r}")
                previous_end = len(stream)
                try:
                    data = os.read(master, 65536)
                except OSError as error:
                    if error.errno != errno.EIO:
                        raise
                    data = b""
                if not data or proc.poll() is not None:
                    raise RuntimeError(f"{args} exited before {marker!r}: {stream[-500:]!r}")
                stream.extend(data)
                for query in QUERY.finditer(stream, max(0, previous_end - 15)):
                    if query.end() > previous_end:
                        os.write(master, REPLIES[query.group()])
                elapsed_ms = (time.monotonic() - start) * 1000
                if sent_input_at is not None:
                    if marker in stream[sent_input_at:]:
                        return {"first_frame": first_frame_ms, "first_input": elapsed_ms}
                elif marker in stream:
                    if not composer:
                        return {"warm_attach": elapsed_ms}
                    first_frame_ms = elapsed_ms
                    sent_input_at = len(stream)
                    marker = "§".encode()
                    os.write(master, marker)
        finally:
            proc.terminate()
            try:
                proc.wait(timeout=3)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait()
    finally:
        os.close(slave)
        os.close(master)


def measure(binary):
    with tempfile.TemporaryDirectory(prefix="nori-startup-") as directory:
        home = Path(directory).resolve()
        cwd = home / "project"
        cwd.mkdir()
        # Consume initialize and deliberately hold the reply until stdin closes.
        # Drafting must work before an adapter can answer any requests.
        (home / "config.toml").write_text(
            f'agent = "benchmark"\n[projects.{json.dumps(str(cwd))}]\ntrust_level = "trusted"\n'
            '[[agents]]\nname = "Benchmark"\nslug = "benchmark"\n'
            '[agents.distribution.local]\ncommand = "/bin/sh"\nargs = ["-c", "exec /bin/cat >/dev/null"]\n'
        )
        env = {
            "PATH": os.environ.get("PATH", os.defpath),
            "TERM": "xterm-256color", "LANG": "en_US.UTF-8", "HOME": str(home),
            "CODEX_HOME": str(home), "NORI_HOME": str(home), "NORI_NO_ANALYTICS": "1",
        }

        def host(*args):
            return subprocess.run(
                [binary, "host", *args], cwd=cwd, env=env, timeout=TIMEOUT,
                stdin=subprocess.DEVNULL, capture_output=True, text=True, check=True,
            )

        result = terminal_sample(binary, ["--skip-trust-directory"], cwd, env, composer=True)
        try:
            start = time.monotonic()
            host("--port", "0")
            result["cold_host"] = (time.monotonic() - start) * 1000
            host("ls")  # Verify an authenticated operation before accepting the sample.
            result.update(terminal_sample(binary, ["attach"], cwd, env, composer=False))
        finally:
            host("stop")
        return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base", required=True, type=Path)
    parser.add_argument("--head", required=True, type=Path)
    parser.add_argument("--samples", type=int, default=50, help="samples per series (minimum 20 for p95)")
    parser.add_argument("--output", type=Path, help="save raw timings, runner and gate results as JSON")
    args = parser.parse_args()
    if args.samples < 20:
        parser.error("use at least 20 samples per series")
    bins = [args.base.resolve(), args.base.resolve(), args.head.resolve()]
    runs = [[], [], []]
    for binary in set(bins):
        measure(binary)  # Warm binary pages and caches equally before sampling.
    for i in range(args.samples):
        for j in range(3):
            slot = (i + j) % 3
            runs[slot].append(measure(bins[slot]))
    report = {
        "runner": platform.platform(), "binaries": list(map(str, bins)),
        "samples_ms": dict(zip(("base", "repeat", "head"), runs)),
        **compare_samples(*runs),
    }
    if args.output:
        args.output.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({key: report[key] for key in ("metrics", "regressions")}, indent=2))
    return 1 if report["regressions"] else 0


if __name__ == "__main__":
    sys.exit(main())
