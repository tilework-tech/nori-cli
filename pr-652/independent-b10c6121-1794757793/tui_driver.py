#!/usr/bin/env python3
"""Scratch helpers; all tmux interaction goes through the bundled tui scripts.

Import Driver, or use start/send/key/assert/capture/stop CLI subcommands.
Caller owns session cleanup and visual inspection of generated captures.
"""
import argparse
import json
import os
import shlex
import subprocess
import time
from pathlib import Path

SKILLS = Path("/home/sprite/org/workspace/.codex/skills")
TUI = SKILLS / "tui-puppeteering-with-tmux"
CAPTURE = SKILLS / "tui-capture-with-ghostty-web"
SCRATCH = Path(__file__).resolve().parent


def make_home(home, cwd, binary):
    """Create a private isolated host configuration without replacing a file."""
    home, cwd, binary = Path(home).resolve(), Path(cwd).resolve(), Path(binary).resolve()
    home.mkdir(parents=True, exist_ok=True, mode=0o700)
    os.chmod(home, 0o700)
    cwd.mkdir(parents=True, exist_ok=True)
    config = home / "config.toml"
    if not config.exists():
        config.write_text('agent = "mock-model"\n[features]\nmulti_session = true\n'
                          f'[projects.{json.dumps(str(cwd))}]\ntrust_level = "trusted"\n')
    return {"NORI_HOME": str(home), "MOCK_ACP_AGENT_BIN": str(binary.parent / "mock_acp_agent"),
            "NORI_NO_ANALYTICS": "1", "COLORTERM": "truecolor"}


class Driver:
    def __init__(self, session):
        self.session = session

    def _script(self, name, *args, timeout=45):
        return subprocess.run([str(TUI / name), self.session, *map(str, args)],
                              cwd=SCRATCH, check=True, capture_output=True,
                              text=True, timeout=timeout).stdout

    def start(self, argv, cwd, env=None):
        """argv is an argument list; cwd and env are shell-quoted by this helper."""
        launch = ["env", "-u", "NO_COLOR", *(f"{key}={value}" for key, value in (env or {}).items()),
                  *map(str, argv)]
        command = "cd " + shlex.quote(str(cwd)) + " && exec " + shlex.join(launch)
        return self._script("tui-start", command)

    def text(self, text):
        return self._script("tui-send", text)

    def key(self, key):
        result = self._script("tui-send", "--keys", key)
        if key == "Escape":
            time.sleep(0.25)
        return result

    def send(self, text):
        self.text(text)
        time.sleep(0.6)  # Let the real composer's paste burst finish before Enter.
        return self.key("Enter")

    def expect(self, text, timeout=15, regex=False):
        args = ["-E", text, str(timeout)] if regex else [text, str(timeout)]
        return self._script("tui-assert", *args, timeout=timeout + 5)

    def screen(self, ansi=False):
        return self._script("tui-capture", *(["-e"] if ansi else []))

    def capture(self, directory, expect, color=False):
        """Capture 120x40 and repair final delimiter; inspect PNG after returning."""
        self.expect(expect)
        directory = Path(directory).resolve()
        command = [str(CAPTURE / "scripts/capture-session"), self.session, str(directory),
                   "--cols", "120", "--rows", "40", "--expect", expect]
        if color:
            command.append("--require-color")
        subprocess.run(command, cwd=SCRATCH, check=True, timeout=180)
        ansi = directory / "screen.ansi"
        raw = ansi.read_bytes()
        (directory / "screen.raw.ansi").write_bytes(raw)
        if raw.endswith(b"\n"):
            ansi.write_bytes(raw[:-1])
            render = subprocess.run(["bun", str(CAPTURE / "scripts/render-ansi.ts"),
                "--input", str(ansi), "--output", str(directory / "screen.png"),
                "--cols", "120", "--rows", "40"], cwd=SCRATCH, check=True,
                capture_output=True, text=True, timeout=180)
            (directory / "metadata.json").write_text(render.stdout)
        return directory

    def stop(self):
        return self._script("tui-stop")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("session")
    subs = parser.add_subparsers(dest="action", required=True)
    start = subs.add_parser("start")
    start.add_argument("--cwd", required=True)
    start.add_argument("--home", required=True)
    start.add_argument("command", nargs=argparse.REMAINDER)
    for action in ["send", "text", "key", "assert"]:
        sub = subs.add_parser(action)
        sub.add_argument("text")
        if action == "assert":
            sub.add_argument("--timeout", type=int, default=15)
    capture = subs.add_parser("capture")
    capture.add_argument("directory")
    capture.add_argument("--expect", required=True)
    capture.add_argument("--color", action="store_true")
    subs.add_parser("screen")
    subs.add_parser("stop")
    args = parser.parse_args()
    driver = Driver(args.session)
    if args.action == "start":
        argv = args.command[1:] if args.command[:1] == ["--"] else args.command
        if not argv:
            parser.error("start needs an executable after --")
        print(driver.start(argv, args.cwd, make_home(args.home, args.cwd, argv[0])))
    elif args.action == "assert":
        print(driver.expect(args.text, args.timeout))
    elif args.action == "capture":
        print(driver.capture(args.directory, args.expect, args.color))
    elif args.action in {"screen", "stop"}:
        print(getattr(driver, args.action)())
    else:
        print(getattr(driver, args.action)(args.text))
