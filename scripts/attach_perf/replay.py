#!/usr/bin/env python3
"""Measure the alternate-screen client with a real host and a draining tmux PTY."""
import argparse
import contextlib
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shlex
import subprocess
import tempfile
import time
import uuid

from fixture import HostFixture

CLOCK = time.monotonic_ns
SYNC = b"\x1b[?2026h"
SECTION = re.compile(r"SectionBeacon(\d{4})(?!\d)")
PAGER_TITLE = "T R A N S C R I P T"


def elapsed(start):
    return round((CLOCK() - start) / 1e6, 3)


class Terminal:
    def __init__(self, root, wrapper, fixture, command, *, respond_to_color=True):
        self.name = "replay-" + uuid.uuid4().hex
        self.tmux = [wrapper] if wrapper else ["tmux", "-f", "/dev/null", "-S", str(root / "tmux")]
        self.raw = root / "terminal.bin"
        self.raw.touch()
        self.gate = root / "launch"
        os.mkfifo(self.gate)
        launch = shlex.join(["env", "-i", *[f"{k}={v}" for k, v in fixture.env.items()], *command])
        shell = f"read -r _ < {shlex.quote(str(self.gate))}; exec {launch}"
        self.call("new-session", "-d", "-s", self.name, "-x", "120", "-y", "40",
                  "-c", str(fixture.cwd), shell)
        self.call("set-option", "-t", self.name, "status", "off")
        # Detached tmux can answer OSC 10/11 only with explicit default colors.
        if respond_to_color:
            for option in ("window-style", "window-active-style"):
                self.call("set-option", "-w", "-t", self.name, option, "fg=#eeeeee,bg=#111111")
        self.call("pipe-pane", "-O", "-t", self.name, "cat >> " + shlex.quote(str(self.raw)))

    def call(self, *args):
        return subprocess.check_output([*self.tmux, *args], text=True, timeout=5)

    def launch(self):
        with self.gate.open("w") as gate:
            start = CLOCK()
            gate.write("start\n")
        return start

    def close(self):
        subprocess.run([*self.tmux, "kill-session", "-t", self.name],
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

    def screen(self):
        return self.call("capture-pane", "-p", "-t", self.name)

    def wait(self, predicate, timeout=120):
        end = time.monotonic() + timeout
        while True:
            screen = self.screen()
            if predicate(screen):
                return screen
            if time.monotonic() >= end:
                raise RuntimeError(f"Timed out observing terminal:\n{screen}")
            time.sleep(0.002)

    def settled(self):
        prior = None
        def complete(screen):
            nonlocal prior
            same = prior == screen
            prior = screen
            with self.raw.open("rb") as raw:
                raw.seek(max(0, self.raw.stat().st_size - 4096))
                tail = raw.read()
            return same and tail.rfind(b"\x1b[?2026l") > tail.rfind(SYNC)
        return self.wait(complete)

    def send(self, key, literal=False):
        self.call("send-keys", "-t", self.name, *(["-l"] if literal else []), key)


def measure(fix, binary, args, directory):
    with contextlib.ExitStack() as stack:
        command = [str(binary), "attach"]
        if args.profile:
            command = ["perf", "record", "-q", "-e", "cpu-clock:u", "-F", "199",
                       "--user-callchains", "--call-graph", "dwarf,8192",
                       "--no-buildid-cache", "--no-buildid", "-o", str(args.profile.resolve()),
                       "--", *command]
        term = Terminal(directory, args.tmux_wrapper, fix, command, respond_to_color=not args.unanswered_colors)
        stack.callback(term.close)
        start = term.launch()
        term.wait(lambda s: f"Sessions {args.sessions}" in s and "enter" in s.lower())
        row = {"launch_to_list_ms": elapsed(start)}
        assert term.call("display-message", "-p", "-t", term.name, "#{alternate_on}").strip() == "1"
        if args.sessions > 1:
            term.send("C-f")
            term.send(fix.title, literal=True)
            term.wait(lambda s: fix.title in s and "Filler sample" not in s)
        raw_offset = term.raw.stat().st_size
        start = CLOCK()
        term.send("Enter")
        def has_history(screen):
            body = "\n".join(screen.splitlines()[3:-4])
            return "enter open" not in body.lower() and (
                SECTION.search(body) or fix.first_marker in body or fix.fence_marker in body
            )
        first_screen = term.wait(has_history)
        row["selection_to_content_ms"] = elapsed(start)
        draft_start = CLOCK()
        term.send("§", literal=True)
        if args.live:
            live_marker = fix.live()
        seen = {}
        for key, marker in [("selection_to_latest_ms", fix.last_marker), ("selection_to_fence_ms", fix.fence_marker)]:
            if marker in first_screen:
                seen[key] = row["selection_to_content_ms"]
        def check(screen):
            for key, marker in markers.items():
                if key not in seen and marker in screen:
                    seen[key] = elapsed(start)
            return len(seen) == len(markers)
        markers = {"selection_to_latest_ms": fix.last_marker, "selection_to_fence_ms": fix.fence_marker,
                   "selection_to_draft_ms": "§"}
        if args.live:
            markers["selection_to_live_ms"] = live_marker
        final = term.wait(check)
        row.update(seen)
        row["catchup_key_ms"] = round(seen["selection_to_draft_ms"] - (draft_start - start) / 1e6, 3)
        capture = term.raw.read_bytes()[raw_offset:]
        row.update(replay_draws=capture.count(SYNC), replay_terminal_bytes=len(capture))
        body = lambda screen: "\n".join(screen.splitlines()[3:-4]).strip()
        final = term.settled()
        row["body_sha256"] = hashlib.sha256(body(final).encode()).hexdigest()
        if args.screen:
            args.screen.write_text(final)
        start = CLOCK()
        term.send("¶", literal=True)
        term.wait(lambda s: "¶" in s)
        row["settled_key_ms"] = elapsed(start)
        page = final
        row["page_up_ms"] = None
        if fix.first_marker not in body(final):
            start = CLOCK()
            term.send("PageUp")
            page = term.wait(lambda s: body(s) != body(final))
            row["page_up_ms"] = elapsed(start)
        if args.stream:
            if row["page_up_ms"] is not None:
                # A client that pages through a transcript pager closes it with q;
                # one that scrolls in place scrolls back down.
                term.send("q" if PAGER_TITLE in page else "PageDown")
                term.wait(lambda s: PAGER_TITLE not in s and fix.fence_marker in s)
            stream_start = CLOCK()
            stream_marker = fix.stream(24, 20)
            term.wait(lambda s: "step 1 " in s)
            key_start = CLOCK()
            term.send("ø", literal=True)
            term.wait(lambda s: "ø" in s)
            row["stream_key_ms"] = elapsed(key_start)
            term.wait(lambda s: stream_marker in s)
            row["stream_total_ms"] = elapsed(stream_start)
            page = term.settled()
        if args.audit:
            numbers = set()
            for screen in (final, page):
                numbers.update(map(int, SECTION.findall(body(screen))))
            for _ in range(args.turns * 20 + 50):
                if fix.first_marker in body(page):
                    break
                previous = body(page)
                term.send("PageUp")
                page = term.wait(lambda s: body(s) != previous)
                # capture-pane can sample tmux in the middle of processing a large update.
                page = term.settled()
                seqs = list(map(int, SECTION.findall(body(page))))
                assert seqs == sorted(set(seqs)), seqs
                numbers.update(seqs)
            assert fix.first_marker in page
            assert numbers == set(range(1, args.turns + 1)), numbers
            row["audited_sections"] = len(numbers)
        if args.check_batching and row["replay_draws"] >= fix.counts["acp_updates"] // 2:
            raise AssertionError(f"Replay used {row['replay_draws']} frames for {fix.counts['acp_updates']} updates")
        if args.profile:
            term.send("C-d")
            deadline = time.monotonic() + 10
            while time.monotonic() < deadline and subprocess.run(
                [*term.tmux, "has-session", "-t", term.name], capture_output=True
            ).returncode == 0:
                time.sleep(0.02)
        return row


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("binary", type=Path)
    p.add_argument("--turns", type=int, default=24)
    p.add_argument("--sessions", type=int, default=1)
    p.add_argument("--samples", type=int, default=5)
    p.add_argument("--tmux-wrapper", default=os.environ.get("NORI_BENCH_TMUX"))
    p.add_argument("--unanswered-colors", action="store_true", help="leave detached tmux colors unspecified")
    p.add_argument("--live", action="store_true")
    p.add_argument("--check-batching", action="store_true", help="reject frame-per-event processing on burst fixtures")
    p.add_argument("--stream", action="store_true")
    p.add_argument("--audit", action="store_true")
    p.add_argument("--profile", type=Path)
    p.add_argument("--screen", type=Path)
    p.add_argument("--output", type=Path)
    args = p.parse_args()
    binary = args.binary.resolve()
    result = {"platform": platform.platform(), "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
              "sessions": args.sessions, "turns": args.turns, "live": args.live, "samples": []}
    for _ in range(args.samples):
        with HostFixture(binary, turns=args.turns, sessions=args.sessions) as fix:
            with tempfile.TemporaryDirectory(prefix="replay-tty-") as tmp:
                row = measure(fix, binary, args, Path(tmp))
            row["fixture"] = fix.counts
            result["samples"].append(row)
            print(json.dumps(row), flush=True)
            if args.output:
                args.output.write_text(json.dumps(result, indent=2) + "\n")


if __name__ == "__main__":
    main()
