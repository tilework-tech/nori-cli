"""Fixture and terminal for the #769 attach replay verification.

A KilnHost owns a fresh synthetic home, a real `nori host`, and one seeded
session produced by kiln_agent.py. A Pane is a private tmux server pane that
drains the PTY continuously; observations read tmux's interpreted cells.
"""
import json
import os
import shlex
import shutil
import subprocess
import sys
import tempfile
import time
import uuid
from pathlib import Path

HERE = Path(__file__).resolve().parent
NS = time.monotonic_ns
SYNC_ON = b"\x1b[?2026h"


def ms_since(t0):
    return round((NS() - t0) / 1e6, 2)


class KilnHost:
    def __init__(self, nori, sections, native_load=False, extra_rows=0):
        self.nori = str(Path(nori).resolve(strict=True))
        self.sections, self.native_load, self.extra_rows = sections, native_load, extra_rows

    def run(self, *args, timeout=60):
        out = subprocess.run([self.nori, "host", *args], cwd=self.cwd, env=self.env,
                             stdin=subprocess.DEVNULL, capture_output=True, text=True,
                             timeout=timeout)
        if out.returncode:
            raise RuntimeError(f"nori host {args} -> {out.returncode}: {out.stderr}")
        return out.stdout.strip()

    def __enter__(self):
        self.tmp = Path(tempfile.mkdtemp(prefix=".kiln-"))
        self.home = self.tmp / "home"
        self.ctrl = self.tmp / "ctrl"
        self.cwd = self.tmp / "workbench"
        for d in (self.home, self.ctrl, self.cwd):
            d.mkdir()
        (self.ctrl / "sections").write_text(str(self.sections))
        if self.native_load:
            (self.ctrl / "native-load").touch()
        self.env = {"HOME": str(self.home), "NORI_HOME": str(self.home),
                    "CODEX_HOME": str(self.home), "NORI_NO_ANALYTICS": "1",
                    "TERM": "xterm-256color", "LANG": "C.UTF-8",
                    "PATH": os.environ.get("PATH", os.defpath)}
        cap = self.extra_rows + 4
        (self.home / "config.toml").write_text(
            f'agent = "kiln"\n[projects.{json.dumps(str(self.cwd))}]\ntrust_level = "trusted"\n'
            f'[tui]\nmax_sessions = {cap}\n[host]\nmax_sessions = {cap}\n'
            '[[agents]]\nname = "Kiln"\nslug = "kiln"\n'
            f'[agents.distribution.local]\ncommand = {json.dumps(sys.executable)}\n'
            f'args = {json.dumps(["-I", str(HERE / "kiln_agent.py"), str(self.ctrl)])}\n')
        self.run("--port", "0")
        self.started = True
        self.session_id = self.run("new", "--agent", "kiln", "--prompt", "kiln seed history")
        for n in range(self.extra_rows):
            self.run("new", "--agent", "kiln", "--prompt", f"filler row {n}")
        deadline = time.monotonic() + 60
        while time.monotonic() < deadline:
            if self.seeded() and self.run("ls").lower().count("ready") >= 1 + self.extra_rows:
                path = next((self.home / "transcripts").rglob(f"{self.session_id}.jsonl"), None)
                if path and "KilnFinalTool" in path.read_text():
                    last = [json.loads(l) for l in path.read_text().splitlines() if l][-1]
                    status = (last.get("event", {}).get("event", {}).get("update", {})
                              .get("_meta", {}).get("nori", {}).get("status"))
                    if status == "idle":
                        break
            time.sleep(0.03)
        else:
            raise RuntimeError("seed never settled: " + self.run("ls"))
        return self

    def ledger(self):
        p = self.ctrl / "ledger.jsonl"
        return [json.loads(l) for l in p.read_text().splitlines()] if p.exists() else []

    def seeded(self):
        return any(e["event"] == "seeded" for e in self.ledger())

    def live(self, n, chunks=1, interval_ms=0):
        tmp = self.ctrl / f".live-{n}"
        tmp.write_text(json.dumps([chunks, interval_ms]))
        tmp.replace(self.ctrl / f"live-{n}")
        return f"KilnLive{n:04d}"

    def __exit__(self, *_):
        try:
            if getattr(self, "started", False):
                subprocess.run([self.nori, "host", "stop"], cwd=self.cwd, env=self.env,
                               capture_output=True, timeout=30)
        finally:
            shutil.rmtree(self.tmp, ignore_errors=True)


class Pane:
    def __init__(self, workdir, host, argv, cols=100, rows=30):
        self.sock = f"kiln-{uuid.uuid4().hex[:10]}"
        self.cols, self.rows = cols, rows
        self.raw = Path(workdir) / "pty.raw"
        self.raw.touch()
        gate = Path(workdir) / "gate"
        os.mkfifo(gate)
        self.gate = gate
        cmd = shlex.join(["env", "-i", *[f"{k}={v}" for k, v in host.env.items()], *argv])
        self.tmux("new-session", "-d", "-s", "k", "-x", str(cols), "-y", str(rows),
                  "-c", str(host.cwd), f"read -r _ < {shlex.quote(str(gate))}; exec {cmd}")
        self.tmux("set-option", "-t", "k", "status", "off")
        for opt in ("window-style", "window-active-style"):
            self.tmux("set-option", "-w", "-t", "k", opt, "fg=#dddddd,bg=#141414")
        self.tmux("pipe-pane", "-O", "-t", "k", "cat >> " + shlex.quote(str(self.raw)))

    def tmux(self, *args):
        return subprocess.check_output(["tmux", "-f", "/dev/null", "-L", self.sock, *args],
                                       text=True, timeout=10)

    def start(self):
        with self.gate.open("w") as g:
            t0 = NS()
            g.write("go\n")
        return t0

    def screen(self):
        return self.tmux("capture-pane", "-p", "-t", "k")

    def ansi(self):
        return self.tmux("capture-pane", "-p", "-e", "-t", "k")

    def key(self, *keys):
        self.tmux("send-keys", "-t", "k", *keys)

    def text(self, s):
        self.tmux("send-keys", "-t", "k", "-l", s)

    def wait(self, pred, timeout=60):
        end = time.monotonic() + timeout
        while True:
            s = self.screen()
            if pred(s):
                return s
            if time.monotonic() > end:
                raise TimeoutError("terminal never matched:\n" + s)
            time.sleep(0.002)

    def settle(self, quiet=0.25, timeout=60):
        """Screen unchanged and raw output idle for `quiet` seconds."""
        end = time.monotonic() + timeout
        prev, size, since = None, -1, time.monotonic()
        while time.monotonic() < end:
            s, n = self.screen(), self.raw.stat().st_size
            if s != prev or n != size:
                prev, size, since = s, n, time.monotonic()
            elif time.monotonic() - since >= quiet:
                return s
            time.sleep(0.01)
        raise TimeoutError("terminal never settled")

    def close(self):
        subprocess.run(["tmux", "-L", self.sock, "kill-server"], capture_output=True)
