"""Real terminals for acceptance runs: one tmux pane per client.

Each pane runs a clean bash so a scenario can observe what the TUI leaves
behind after it exits (alternate screen, cursor, tty modes, scrollback).
All raw PTY output is also logged with pipe-pane for escape-sequence checks.
"""
import re
import shlex
import os
import subprocess
import time
from pathlib import Path

SOCKET = "nori-host-tui-acceptance" + (f"-{os.environ['ACC_LANE']}" if os.environ.get("ACC_LANE") else "")
PROMPT = "shell$ "


def tmux(*args, check=True):
    return subprocess.run(["tmux", "-f", "/dev/null", "-L", SOCKET, *args],
                          capture_output=True, text=True, check=check)


class Term:
    def __init__(self, name, env, cwd, out_dir: Path, cols=120, rows=36):
        self.name, self.cols, self.rows = name, cols, rows
        self.out = out_dir
        self.out.mkdir(parents=True, exist_ok=True)
        self.raw_log = self.out / f"{name}.raw"
        exports = " ".join(f"{k}={shlex.quote(v)}" for k, v in env.items())
        shell = f"env -i {exports} PS1={shlex.quote(PROMPT)} bash --norc --noprofile"
        tmux("kill-session", "-t", name, check=False)
        tmux("new-session", "-d", "-s", name, "-x", str(cols), "-y", str(rows), "-c", str(cwd), shell)
        tmux("set-option", "-t", name, "status", "off")
        tmux("set-option", "-t", name, "history-limit", "5000")
        tmux("pipe-pane", "-t", name, "-o", f"cat >> {shlex.quote(str(self.raw_log))}")
        self.wait_for(PROMPT.strip())

    # ---- input -------------------------------------------------------
    def type(self, text):
        tmux("send-keys", "-t", self.name, "-l", text)
        time.sleep(0.15)

    def keys(self, *keys):
        for key in keys:
            tmux("send-keys", "-t", self.name, key)
            time.sleep(0.12)

    def run(self, command):
        self.type(command)
        self.keys("Enter")

    # ---- observation -------------------------------------------------
    def screen(self, ansi=False):
        args = ["capture-pane", "-p", "-t", self.name]
        if ansi:
            args.append("-e")
        return tmux(*args).stdout

    def history(self):
        return tmux("capture-pane", "-p", "-S", "-5000", "-t", self.name).stdout

    def fmt(self, expr):
        return tmux("display-message", "-p", "-t", self.name, expr).stdout.strip()

    def alt_screen(self):
        return self.fmt("#{alternate_on}") == "1"

    def cursor_visible(self):
        return self.fmt("#{cursor_flag}") == "1"

    def title(self):
        return self.fmt("#{pane_title}")

    def raw(self):
        return self.raw_log.read_bytes() if self.raw_log.exists() else b""

    def wait_for(self, needle, timeout=15, regex=False):
        deadline = time.time() + timeout
        while time.time() < deadline:
            text = self.screen()
            if (re.search(needle, text) if regex else needle in text):
                return text
            time.sleep(0.2)
        raise TimeoutError(f"{self.name}: {needle!r} not on screen\n{self.screen()}")

    def wait_gone(self, needle, timeout=10):
        deadline = time.time() + timeout
        while time.time() < deadline:
            if needle not in self.screen():
                return True
            time.sleep(0.2)
        return False

    def at_shell(self):
        """True when the TUI has exited and bash shows its prompt again."""
        lines = [l for l in self.screen().splitlines() if l.strip()]
        return bool(lines) and lines[-1].startswith(PROMPT.strip())

    def tty_modes(self):
        """Run `stty -a` in the restored shell and return the flags line."""
        marker = f"STTY{int(time.time() * 1000)}"
        self.run(f"echo {marker}; stty -a | tr '\\n' ' '; echo; echo END{marker}")
        text = self.wait_for(f"END{marker}")
        return text

    def capture(self, label, expect=None):
        """Save text and ANSI of the visible screen under out/<label>.*"""
        if expect:
            self.wait_for(expect)
        (self.out / f"{label}.txt").write_text(self.screen())
        ansi = self.screen(ansi=True)
        (self.out / f"{label}.ansi").write_text(ansi[:-1] if ansi.endswith("\n") else ansi)
        return self.out / f"{label}.ansi"

    def close(self):
        tmux("kill-session", "-t", self.name, check=False)


def kill_server():
    tmux("kill-server", check=False)


ESCAPES = {
    "alt_on": b"\x1b[?1049h", "alt_off": b"\x1b[?1049l",
    "paste_on": b"\x1b[?2004h", "paste_off": b"\x1b[?2004l",
    "cursor_hide": b"\x1b[?25l", "cursor_show": b"\x1b[?25h",
    "kbd_push": b"\x1b[>", "kbd_pop": b"\x1b[<",
    "focus_on": b"\x1b[?1004h", "focus_off": b"\x1b[?1004l",
}


def escape_counts(raw: bytes):
    return {name: raw.count(seq) for name, seq in ESCAPES.items()}
