#!/usr/bin/env python3
"""Independent #773 smoke: first interactive key and populated attach picker.

A kiln host holds one ready synthetic session (8 sections). `nori attach`
runs on a raw controlling PTY drained continuously. The probe answers cursor
position (CSI 6n), status (CSI 5n) and device attributes (CSI c) at once and
answers OSC 10/11 colour queries immediately, after 150 ms, or never. The
first keystroke, a lone "ж", is written as soon as the first query arrives.
Echo is off, so the key counts as handled only when the app renders it.
After the picker is populated the probe clears the key, presses Enter and
waits for the saved session's last tool to render, then Ctrl+D quits.
"""
import argparse
import json
import os
import pty
import re
import select
import signal
import statistics
import subprocess
import sys
import termios
import time
import tty
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from kiln import KilnHost  # noqa: E402

QUERY = re.compile(rb"\x1b(?:\[(5n|6n|c)|\](10|11);\?(?:\x1b\\|\x07))")
KEY = "ж".encode()
MODES = {"immediate": 0.0, "delayed": 0.150, "missing": None}
REPLY = {b"10": b"rgb:eeee/eeee/eeee", b"11": b"rgb:1414/1414/1414"}


def sample(binary, mode):
    with KilnHost(binary, 8) as host:
        master, slave = pty.openpty()
        termios.tcsetwinsize(slave, (30, 100))
        tty.setraw(slave)
        attrs = termios.tcgetattr(slave)
        attrs[3] &= ~termios.ECHO
        termios.tcsetattr(slave, termios.TCSANOW, attrs)
        env = dict(host.env, COLORTERM="truecolor")
        t0 = time.monotonic()
        proc = subprocess.Popen([str(binary), "attach"], stdin=slave, stdout=slave, stderr=slave,
                                cwd=host.cwd, env=env, start_new_session=True,
                                preexec_fn=lambda: __import__("fcntl").ioctl(0, termios.TIOCSCTTY, 0))
        os.close(slave)
        out, scanned, pending, marks, queries = bytearray(), 0, [], {}, []
        delay = MODES[mode]

        def ms():
            return round((time.monotonic() - t0) * 1000, 1)

        def pump(until, timeout=10.0):
            nonlocal scanned
            end = time.monotonic() + timeout
            while time.monotonic() < end:
                now = time.monotonic()
                for item in [p for p in pending if p[0] <= now]:
                    pending.remove(item)
                    os.write(master, item[1])
                r, _, _ = select.select([master], [], [], 0.005)
                if r:
                    try:
                        out.extend(os.read(master, 65536))
                    except OSError:
                        return False
                for m in QUERY.finditer(bytes(out), max(0, scanned - 16)):
                    if m.end() <= scanned:
                        continue
                    kind = m.group(1) or m.group(2)
                    queries.append([kind.decode(), ms()])
                    if kind == b"6n":
                        os.write(master, b"\x1b[1;1R")
                    elif kind == b"5n":
                        os.write(master, b"\x1b[0n")
                    elif kind == b"c":
                        os.write(master, b"\x1b[?62;22c")
                    elif delay is not None:
                        pending.append((time.monotonic() + delay,
                                        b"\x1b]" + kind + b";" + REPLY[kind] + b"\x1b\\"))
                    if "key_sent" not in marks:
                        marks["key_sent"] = ms()
                        os.write(master, KEY)
                scanned = len(out)
                if until():
                    return True
            return False

        result = {"mode": mode}
        try:
            plain = lambda: re.sub(rb"\x1b\[[0-?]*[ -/]*[@-~]", b"", bytes(out))  # noqa: E731
            result["key_handled"] = pump(lambda: KEY in plain())
            marks["key_handled"] = ms()
            result["picker_populated"] = pump(lambda: b"kiln seed history" in plain() or b"Sessions 1" in plain())
            marks["picker_populated"] = ms()
            pump(lambda: False, 0.2)
            os.write(master, b"\x7f")
            pump(lambda: False, 0.2)
            offset = len(out)
            os.write(master, b"\r")
            result["opened_session"] = pump(lambda: b"final.txt" in re.sub(
                rb"\x1b\[[0-?]*[ -/]*[@-~]", b"", bytes(out[offset:])))
            marks["session_content"] = ms()
            os.write(master, b"\x04")
            pump(lambda: proc.poll() is not None, 5)
        finally:
            if proc.poll() is None:
                os.killpg(proc.pid, signal.SIGKILL)
            proc.wait()
            os.close(master)
        result.update(ms=marks, exit_code=proc.returncode, queries=queries[:12],
                      osc_queries=sum(q[0] in ("10", "11") for q in queries))
        return result


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--build", nargs=2, action="append", metavar=("LABEL", "BINARY"), required=True)
    p.add_argument("--samples", type=int, default=5)
    p.add_argument("--out", type=Path, required=True)
    a = p.parse_args()
    rows = []
    for i in range(a.samples):
        for mode in MODES:
            for label, b in (a.build if i % 2 == 0 else a.build[::-1]):
                r = sample(Path(b).resolve(), mode)
                r.update(build=label, index=i)
                rows.append(r)
                print(json.dumps(r), flush=True)
                a.out.write_text(json.dumps(rows, indent=1) + "\n")
    summary = {}
    for label, _ in a.build:
        for mode in MODES:
            mine = [r for r in rows if r["build"] == label and r["mode"] == mode]
            summary[f"{label} {mode}"] = {
                "n": len(mine),
                "key_handled_median_ms": statistics.median(r["ms"]["key_sent"] and r["ms"]["key_handled"] for r in mine),
                "picker_median_ms": statistics.median(r["ms"]["picker_populated"] for r in mine),
                "all_ok": all(r["key_handled"] and r["picker_populated"] and r["opened_session"] for r in mine)}
    print(json.dumps(summary, indent=1))
    a.out.write_text(json.dumps({"summary": summary, "samples": rows}, indent=1) + "\n")


if __name__ == "__main__":
    main()
