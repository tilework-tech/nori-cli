"""Probe a real foreground editor and an interactive shell job-control handoff."""
import argparse
import json
import os
from pathlib import Path
import signal
import sys

from measure import ESC, SECOND, Terminal, plain_fixture


def send_owner_line(term):
    term.write(b"owner-input")
    # Keep the line open across pending emulator responses so leakage is observable.
    while term.pending:
        term.pump()
    term.write(b"\n")


def editor(binary, fixtures, mode):
    with plain_fixture(fixtures) as (cwd, env):
        home = Path(env["HOME"])
        helper = home / "editor.py"
        helper.write_text(
            "#!" + sys.executable + "\n"
            "import json, os, sys, termios\nfrom pathlib import Path\n"
            "print('SyntheticEditorOwnsTerminal', flush=True)\n"
            "line = sys.stdin.buffer.readline()\n"
            "Path(os.environ['HOME'], 'editor.json').write_text(json.dumps({"
            "'canonical': bool(termios.tcgetattr(0)[3] & termios.ICANON), "
            "'typed': b'owner-input' in line, 'osc': line.count(b';rgb:'), "
            "'operating_status': line.count(b'\\x1b[0n')}))\n"
            "Path(sys.argv[-1]).write_text('ReturnedFromEditor')\n")
        helper.chmod(0o700)
        term = Terminal(binary, ["--skip-trust-directory"], cwd,
                        dict(env, COLORTERM="truecolor", VISUAL=str(helper)), mode)
        try:
            term.see("ready", b"CobaltReady")
            term.mark("shortcut")
            term.write(b"\x07")
            term.see("editor", b"SyntheticEditorOwnsTerminal")
            send_owner_line(term)
            term.see("resumed", b"ReturnedFromEditor")
            term.write(SECOND)
            term.see("handled_after_resume", SECOND)
            term.write(b"\x03\x04")
            restored = term.wait_exit()
            return {"time_ms": term.milestones, "restored": restored, **json.loads((home / "editor.json").read_text())}
        finally:
            term.close()


def suspend(binary, host_fixture, mode):
    with host_fixture(binary, turns=1, sessions=2) as host:
        # Interactive bash owns the controlling terminal and puts the attached
        # Nori in a separate foreground process group, as a real shell does.
        commands = ('set -m; "$PALETTE_CLI" attach; printf "\\nSyntheticShellOwnsTerminal\\n"; '
                    'IFS= read -r bytes; printf %s "$bytes" >"$HOME/shell-input"; fg; '
                    'printf "\\nSyntheticJobCompleted\\n"')
        term = Terminal("/bin/bash", ["--noprofile", "--norc", "-ic", commands], host.cwd,
                        dict(host.env, PALETTE_CLI=str(binary), COLORTERM="truecolor"), mode)
        try:
            term.see("ready", b"Sessions 2")
            term.mark("shortcut")
            term.write(b"\x1a")
            term.see("shell", b"SyntheticShellOwnsTerminal")
            if os.tcgetpgrp(term.master) != term.proc.pid:
                raise AssertionError("Bash did not own the terminal while Nori was suspended")
            offset = len(term.output)
            send_owner_line(term)
            term.see("resumed", b"Sessions 2", since=offset)
            term.write(SECOND)
            term.see("handled_after_resume", SECOND, since=offset)
            term.write(b"\x04")
            term.see("job_complete", b"SyntheticJobCompleted")
            restored = term.wait_exit()
            line = (host.home / "shell-input").read_bytes()
            return {"time_ms": term.milestones, "restored": restored, "typed": b"owner-input" in line,
                    "osc": line.count(b";rgb:"), "operating_status": line.count(ESC + b"[0n")}
        finally:
            group = os.tcgetpgrp(term.master)
            if group > 0 and group != term.proc.pid:
                try:
                    if os.getsid(group) == term.proc.pid:
                        os.killpg(group, signal.SIGKILL)
                except ProcessLookupError:
                    pass
            term.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--fixtures", required=True, type=Path)
    parser.add_argument("--samples", type=int, default=3)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    fixtures = args.fixtures.resolve()
    sys.path.insert(0, str(fixtures))
    from fixture import HostFixture
    binary = args.binary.resolve()
    results = {"editor": [], "suspend": []}
    for _ in range(args.samples):
        for kind in results:
            result = (editor(binary, fixtures, "delayed") if kind == "editor"
                      else suspend(binary, HostFixture, "delayed"))
            results[kind].append(result)
            args.output.write_text(json.dumps(results, indent=2) + "\n")
            print(kind, result, flush=True)
            if args.check and (not result["restored"] or not result["typed"] or result["osc"]
                               or result["operating_status"] or not result.get("canonical", True)):
                raise AssertionError(f"Failed foreground ownership check on {kind}")


if __name__ == "__main__":
    main()
