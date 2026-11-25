#!/usr/bin/env python3
"""Observe first handled input and palette responses on a real controlling PTY."""
import argparse
from collections import deque
import contextlib
import fcntl
import hashlib
import json
import os
from pathlib import Path
import platform
import pty
import re
import select
import shlex
import signal
import struct
import subprocess
import sys
import tempfile
import termios
import time

ESC = b"\x1b"
ST = ESC + b"\\"
QUERY = re.compile(rb"\x1b(?:\[(?:5n|6n|c)|\](10|11);\?(?:\x1b\\|\x07))")
EARLY = "δ".encode()
SECOND = "§".encode()
FOCUSED = "β".encode()
# Light 0xe0d0c0. The four-percent stripe is RGB(215,199,184).
STRIPE = b"48;2;215;199;184"
COLOR = {b"10": b"1212/3434/5656", b"11": b"e0e0/d0d0/c0c0"}
SHELL_INPUT = b"next-owner"
MODES = {"immediate": 0, "delayed": .150, "missing": None, "very-late": .450, "silent-barrier": None, "silent-all": None, "background-only": .150, "device-silent": 0}
PATHS = ("attach", "attach-id", "attach-remote", "resume", "plain", "plain-pipe", "cloud", "host-ls", "host-ls-pipe", "attach-exit", "resume-exit", "fullscreen-exit")
EXITS = {"attach-exit", "resume-exit", "fullscreen-exit"}


def launch_tty():
    os.setsid()
    fcntl.ioctl(1, termios.TIOCSCTTY, 0)


class Terminal:
    def __init__(self, binary, args, cwd, env, mode, *, pipe_stdin=False):
        self.master, self.slave = pty.openpty()
        fcntl.ioctl(self.slave, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 120, 0, 0))
        self.original = termios.tcgetattr(self.slave)
        self.original[3] &= ~termios.ECHO
        termios.tcsetattr(self.slave, termios.TCSANOW, self.original)
        self.output = bytearray()
        self.pending = deque()
        self.palette = []
        self.focused = False
        self.milestones = {}
        self.mode = mode
        self.delay = MODES[mode]
        self.start = time.monotonic()
        self.proc = subprocess.Popen([binary, *args], stdin=subprocess.PIPE if pipe_stdin else self.slave, stdout=self.slave,
                                     stderr=self.slave, preexec_fn=launch_tty, cwd=cwd, env=env)
        if pipe_stdin:
            self.proc.stdin.write("Synthetic piped prompt §".encode())
            self.proc.stdin.close()
        self.on_palette_query = None
        self.on_first_query = None

    def mark(self, name):
        self.milestones.setdefault(name, round(1000 * (time.monotonic() - self.start), 3))

    def write(self, data):
        os.write(self.master, data)

    def pump(self, maximum=.02):
        now = time.monotonic()
        while self.pending and self.pending[0][0] <= now:
            _, sequence, reply = self.pending.popleft()
            self.write(reply)
            self.mark(("focus_reply_" if self.focused else "reply_") + sequence)
        if self.pending:
            maximum = min(maximum, max(0, self.pending[0][0] - now))
        if not select.select([self.master], [], [], maximum)[0]:
            return
        previous = len(self.output)
        self.output.extend(os.read(self.master, 65536))
        if STRIPE in self.output[max(0, previous - len(STRIPE)):]:
            self.mark("stripe")
        for match in QUERY.finditer(self.output, max(0, previous - 20)):
            if match.end() <= previous:
                continue
            seq = match.group(1)
            if self.on_first_query:
                self.on_first_query()
                self.on_first_query = None
            if self.mode == "silent-all":
                query = match[0]
                name = {"6n": "cursor", "[c": "device", "5n": "status"}.get(query[-2:].decode(), "palette")
                self.mark("query_" + (seq.decode() if seq else name))
                continue
            if self.mode == "device-silent" and match[0] == ESC + b"[c":
                self.mark("query_device")
                if self.on_palette_query:
                    self.on_palette_query()
                continue
            if match[0] == ESC + b"[5n":
                self.mark("status_query")
                if self.mode != "silent-barrier":
                    # A conforming emulator handles the barrier after earlier requests.
                    after_palette = self.pending[-1][0] if self.pending else time.monotonic()
                    self.pending.append((after_palette, "status", ESC + b"[0n"))
                continue
            if seq is None:
                self.write(ESC + (b"[1;1R" if match[0].endswith(b"6n") else b"[?1;2c"))
                continue
            self.palette.append(seq.decode())
            self.mark("query_" + seq.decode())
            if self.on_palette_query:
                self.on_palette_query()
            if self.delay is not None and (self.mode != "background-only" or seq == b"11"):
                self.pending.append((time.monotonic() + self.delay, seq.decode(),
                                     ESC + b"]" + seq + b";rgb:" + (b"2020/3030/4040" if self.focused and seq == b"11" else COLOR[seq]) + ST))

    def until(self, test, *, timeout=7, required=True):
        deadline = time.monotonic() + timeout
        while not test():
            if time.monotonic() > deadline or self.proc.poll() is not None:
                if required:
                    raise RuntimeError(f"missing milestone after {self.milestones}: {bytes(self.output[-650:])!r}")
                return False
            self.pump()
        return True

    def see(self, name, needle, *, since=0, **kwargs):
        if self.until(lambda: self.output.find(needle, since) != -1, **kwargs):
            self.mark(name)
            return True
        return False

    def wait_exit(self):
        self.until(lambda: self.proc.poll() is not None)
        while select.select([self.master], [], [], 0)[0]:
            self.pump(0)
        if self.proc.returncode:
            raise RuntimeError(f"CLI exited {self.proc.returncode}: {self.output[-400:]!r}")
        self.mark("exit")
        return termios.tcgetattr(self.slave) == self.original

    def read_owner_line(self):
        # Linux can return a short read for bytes queued during a raw-to-cooked transition.
        data = bytearray()
        deadline = time.monotonic() + .5
        while b"\n" not in data and time.monotonic() < deadline:
            if select.select([self.slave], [], [], max(0, deadline - time.monotonic()))[0]:
                data.extend(os.read(self.slave, 4096))
        return bytes(data)

    def close(self):
        if self.proc.poll() is None:
            os.killpg(self.proc.pid, signal.SIGKILL)
            self.proc.wait()
        os.close(self.master)
        os.close(self.slave)


@contextlib.contextmanager
def plain_fixture(fixture_dir):
    with tempfile.TemporaryDirectory(prefix="palette-probe-") as directory:
        home = Path(directory)
        cwd = home / "project"
        cwd.mkdir()
        (home / "config.toml").write_text(
            f'agent = "timing-probe"\n[projects.{json.dumps(str(cwd))}]\ntrust_level = "trusted"\n'
            '[[agents]]\nname = "Timing probe"\nslug = "timing-probe"\n[agents.distribution.local]\n'
            f'command = {json.dumps(sys.executable)}\nargs = '
            + json.dumps(["-I", str(fixture_dir / "startup_agent.py"), str(home / "trace"), "0", "4", "0"]) + "\n")
        yield cwd, {"PATH": os.environ["PATH"], "HOME": str(home), "CODEX_HOME": str(home),
                    "NORI_HOME": str(home), "NORI_NO_ANALYTICS": "1",
                    "LANG": "C.UTF-8", "TERM": "xterm-256color"}


def probe(binary, path, mode, fixture_dir, fixture_class):
    with contextlib.ExitStack() as scope:
        if path in ("plain", "plain-pipe", "cloud", "fullscreen-exit"):
            cwd, env = scope.enter_context(plain_fixture(fixture_dir))
            args = ["--skip-trust-directory"]
            if path == "cloud":
                home = Path(env["HOME"])
                adapter = home / "synthetic-handroll"
                launch = [sys.executable, "-I", str(fixture_dir / "startup_agent.py"),
                          str(home / "cloud.trace"), "3", "4", "0"]
                adapter.write_text("#!/bin/sh\nexec " + shlex.join(launch) + "\n")
                adapter.chmod(0o700)
                env["NORI_HANDROLL_BIN"] = str(adapter)
                args = ["cloud"]
        else:
            host = scope.enter_context(fixture_class(binary, turns=1, sessions=2))
            cwd, env = host.cwd, host.env
            endpoint = json.loads((host.home / "host/state.json").read_text())["endpoint"]
            args = {"attach": ["attach"], "attach-id": ["attach", host.session_id],
                    "attach-remote": ["attach", "--host", endpoint, "--token-file", str(host.home / "host/token")],
                    "attach-exit": ["attach"], "resume-exit": ["resume"],
                    "resume": ["resume"], "host-ls": ["host", "ls"],
                    "host-ls-pipe": ["host", "ls"]}[path]
        env = dict(env, COLORTERM="truecolor")
        if path == "host-ls-pipe":
            start = time.monotonic()
            out = subprocess.check_output([binary, *args], stdin=subprocess.DEVNULL, cwd=cwd, env=env)
            if ESC in out or host.session_id.encode() not in out:
                raise ValueError("Pipe contained controls or omitted its session")
            return {"time_ms": {"ready": round((time.monotonic() - start) * 1000, 3)}, "palette_queries": []}

        term = Terminal(binary, args, cwd, env, mode, pipe_stdin=path == "plain-pipe")
        scope.callback(term.close)
        if mode == "silent-all":
            def send_first():
                term.mark("send_early")
                term.write(EARLY)
            term.on_first_query = send_first
            ready = b"CobaltReady" if path == "plain" else b"Sessions 2"
            end = time.monotonic() + 12
            while time.monotonic() < end and term.proc.poll() is None:
                term.pump()
                if ready in term.output:
                    term.mark("ready")
                if EARLY in term.output:
                    term.mark("first_handled")
                    if "ready" in term.milestones:
                        break
            exited = term.proc.poll() is not None
            if exited:
                term.mark("exit")
            if not exited and "ready" in term.milestones and "first_handled" in term.milestones:
                term.write(b"\x03\x04" if path == "plain" else b"\x04")
                term.wait_exit()
            clean = re.sub(rb"\x1b\[[0-?]*[ -/]*[@-~]", b"", bytes(term.output)).decode(errors="replace")
            return {"time_ms": term.milestones, "palette_queries": [], "exit_code": term.proc.poll(),
                    "startup_exited": exited, "restored": termios.tcgetattr(term.slave) == term.original,
                    "canonical_after_exit": bool(termios.tcgetattr(term.slave)[3] & termios.ICANON),
                    "last_output": clean.strip()[-260:] if exited else ""}
        if path in EXITS:
            if path == "fullscreen-exit":
                term.see("ready", b"CobaltReady")
                term.write(b"/screen")
                term.see("command", b"/screen")
                time.sleep(.12)
                term.write(b"\r")
                term.see("fullscreen", ESC + b"[?1049h")
                # Request fresh colors, then exit with those responses still pending.
                offset = len(term.output)
                term.write(ESC + b"[O" + ESC + b"[I")
                term.see("focus_query", ESC + b"]10;?", since=offset)
            else:
                ready = b"Sessions 2" if path == "attach-exit" else b"Resume a previous session"
                term.see("ready", ready)
            offset = len(term.output)
            term.mark("quit")
            term.write(b"\x03" if path == "resume-exit" else b"\x04")
            term.see("shell_visible", ESC + b"[?1049l", since=offset)
            # The shell screen is visible before process exit. Send the next
            # command at that boundary to catch a stale application reader.
            term.write(SHELL_INPUT)
            restored = term.wait_exit()
            waiting = sum(sequence != "status" for _, sequence, _ in term.pending)
            # The next foreground owner receives bytes the emulator writes after Nori exits.
            while term.pending:
                term.pump()
            term.write(b"\n")
            following = term.read_owner_line()
            return {"time_ms": term.milestones, "palette_queries": term.palette, "restored": restored,
                    "replies_outstanding_on_exit": waiting, "next_owner_received_color_replies": following.count(b";rgb:"),
                    "next_owner_received_typeahead": SHELL_INPUT in following,
                    "next_owner_exact_line": following == SHELL_INPUT + b"\n",
                    "next_owner_bytes_hex": following.hex(),
                    "next_owner_received_status_reply": ESC + b"[0n" in following}
        if path == "host-ls":
            term.write(SHELL_INPUT)
            term.see("ready", b"Status")
            restored = term.wait_exit()
            term.write(b"\n")
            remaining = term.read_owner_line()
            if b"Status" not in term.output:
                raise ValueError("Missing host list")
            return {"time_ms": term.milestones, "palette_queries": term.palette, "restored": restored,
                    "next_owner_received_typeahead": remaining == SHELL_INPUT + b"\n",
                    "stripe": STRIPE in term.output}

        def early():
            if "send_early" not in term.milestones:
                term.mark("send_early")
                term.write((b"\x06" if path in ("resume", "cloud") else b"") + EARLY)

        term.on_palette_query = None if path == "cloud" else early
        ready = {"plain": b"CobaltReady", "plain-pipe": b"CobaltReady", "resume": b"Resume a previous session",
                 "cloud": b"CobaltArchive", "attach-remote": b"Sessions 2",
                 "attach": b"Sessions 2", "attach-id": b"CompleteReplayBeacon"}[path]
        term.until(lambda: ready in term.output or EARLY in term.output or
                   (path in ("plain", "plain-pipe") and "›".encode() in term.output))
        early()
        def see_first():
            if ready in term.output:
                term.mark("ready")
            if EARLY in term.output:
                term.mark("first_handled")
            return "ready" in term.milestones and "first_handled" in term.milestones
        # A failed typeahead check is data on the baseline. Keep going and
        # record whether a key sent after the ready frame works.
        term.until(see_first, timeout=2.5, required=False)
        term.see("ready", ready)
        offset = len(term.output)
        term.mark("send_second")
        if path == "resume" and "first_handled" not in term.milestones:
            term.write(b"\x06")
        term.write(SECOND)
        term.see("second_handled", SECOND, since=offset)
        if path in ("plain", "plain-pipe", "attach-id"):
            time.sleep(.12)  # Let the composer finish recognizing a burst of pasted characters.
            if path == "plain-pipe":
                term.see("piped_prompt_answered", b"CobaltReceived")
            submission_offset = len(term.output)
            term.mark("submit")
            term.write(b"\r")
            term.see("answered", b"CobaltReceived" if path != "attach-id" else b"acknowledgement.", since=submission_offset)
            if path == "attach-id":
                sent = [e["text"] for e in host.agent_events() if e["event"] == "prompt"]
                if not sent or sent[-1] != (EARLY + SECOND).decode():
                    raise ValueError(f"Prompt bytes changed before reaching the attached agent: {sent}")
        # Let delayed replies arrive even when the UI became usable first.
        # An independent key after focus exercises the code that refreshes colors.
        if mode == "delayed":
            term.until(lambda: not term.pending, timeout=1)
        if mode != "missing" and path == "attach":
            term.see("stripe", STRIPE, timeout=.4, required=False)
        if mode == "background-only":
            # Allow the outstanding foreground query to expire before requesting the new theme.
            query_at = term.start + term.milestones["query_10"] / 1000
            while time.monotonic() < query_at + .21:
                term.pump()
        term.focused = True
        term.write(ESC + b"[O" + ESC + b"[I")
        offset = len(term.output)
        term.mark("send_focus_key")
        term.write(FOCUSED)
        term.see("focus_handled", FOCUSED, since=offset)
        if mode == "delayed":
            term.until(lambda: not term.pending, timeout=1)
        if mode != "missing" and path == "attach":
            # A theme change on focus now gives RGB(32,48,64); observe its four-percent stripe.
            term.see("focus_stripe", b"48;2;40;56;71", since=offset, timeout=.5, required=False)
        if path == "cloud":
            term.write(b"\x1b")
            term.until(lambda: b"stop search" not in term.output[-150:], timeout=.1, required=False)
            term.write(b"\x03\x04")
        else:
            term.write(b"\x03" if path == "resume" else b"\x04")
        if path in ("plain", "plain-pipe"):
            # Ctrl+D with a draft clears instead of exiting.
            term.write(b"\x03\x04")
        restored = term.wait_exit()
        return {"time_ms": term.milestones, "palette_queries": term.palette, "restored": restored,
                "alternate_screen_paired": term.output.count(ESC + b"[?1049h") == term.output.count(ESC + b"[?1049l"),
                "stripe": STRIPE in term.output, "input_contains_osc_payload": b"rgb:" in term.output}


def check(path, mode, row):
    timings = row["time_ms"]
    if path == "host-ls-pipe":
        return
    if not row["restored"]:
        raise AssertionError("Terminal attributes changed after exit")
    if mode == "silent-all":
        if row["exit_code"] != 1 or not row["canonical_after_exit"] or "cursor position" not in row["last_output"]:
            raise AssertionError("The cursor-position failure did not return the original terminal")
        return
    if path in EXITS:
        if not row["next_owner_received_typeahead"]:
            raise AssertionError("Input lost on interactive exit")
        if mode in ("delayed", "silent-barrier") and not row["next_owner_exact_line"]:
            raise AssertionError("On-time replies reached the next foreground owner")
        return
    if path == "host-ls":
        if row["palette_queries"] or not row["next_owner_received_typeahead"]:
            raise AssertionError("One-shot listing read input owned by the shell")
        return
    if ("first_handled" not in timings or row["input_contains_osc_payload"]
            or not row["alternate_screen_paired"]):
        raise AssertionError("Input was lost, a color response reached the UI, or the screen was not restored")
    if mode == "missing" and timings["first_handled"] > 1000:
        raise AssertionError("First input waited for a missing color response")
    if mode == "delayed" and path == "attach":
        if timings["first_handled"] >= timings["reply_10"] or timings["focus_handled"] >= timings["focus_reply_10"]:
            raise AssertionError("The picker's input waited for a delayed color response")
    if mode != "missing" and path == "attach" and ("stripe" not in timings or "focus_stripe" not in timings):
        raise AssertionError("Reported startup or focus colors never reached the picker")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--fixtures", required=True, type=Path, help="PR 770 scripts/attach_perf directory")
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--base", type=Path, help="alternate with a second binary in the same sample round")
    parser.add_argument("--samples", type=int, default=5)
    parser.add_argument("--paths", nargs="+", choices=PATHS, default=PATHS)
    parser.add_argument("--modes", nargs="+", choices=MODES, default=[mode for mode in MODES if mode not in ("silent-all", "background-only", "device-silent")])
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--check", action="store_true", help="require responsive input, applied colors and terminal ownership")
    args = parser.parse_args()
    fixtures = args.fixtures.resolve()
    sys.path.insert(0, str(fixtures))
    from fixture import HostFixture
    binaries = {"candidate": args.binary.resolve(strict=True)}
    if args.base:
        binaries["base"] = args.base.resolve(strict=True)
    reports = {}
    for name, binary in binaries.items():
        with binary.open("rb") as source:
            digest = hashlib.file_digest(source, "sha256").hexdigest()
        reports[name] = {"platform": platform.platform(), "binary_sha256": digest, "dimensions": [120, 40],
                         "delayed_reply_ms": 150, "very_late_reply_ms": 450, "samples": {}}
    cases = [(path, mode) for path in args.paths for mode in args.modes
             if (path != "host-ls-pipe" or mode == args.modes[0])
             and (mode != "silent-all" or path in ("plain", "attach"))
             and (mode not in ("background-only", "device-silent") or path == "attach")
             and (path in EXITS or mode not in ("very-late", "silent-barrier"))
             and (path != "attach-exit" or mode in ("delayed", "very-late", "silent-barrier"))
             and (path not in ("resume-exit", "fullscreen-exit") or mode == "delayed")]
    if not cases or args.samples < 1:
        parser.error("select at least one applicable path, mode and sample")
    for i in range(args.samples):
        ordered = cases[i % len(cases):] + cases[:i % len(cases)]
        for index, (path, mode) in enumerate(ordered):
            names = list(binaries)
            if (index + i) % 2:
                names.reverse()
            for name in names:
                row = probe(binaries[name], path, mode, fixtures, HostFixture)
                reports[name]["samples"].setdefault(f"{path}/{mode}", []).append(row)
                output = reports if args.base else reports[name]
                args.output.write_text(json.dumps(output, indent=2) + "\n")
                print(name, path, mode, row, flush=True)
                if args.check and name == "candidate":
                    check(path, mode, row)


if __name__ == "__main__":
    main()
