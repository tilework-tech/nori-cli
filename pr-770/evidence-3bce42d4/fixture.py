"""Own one fresh real Nori host seeded by the synthetic ACP subprocess."""

import json
import os
import subprocess
import sys
import tempfile
import time
from pathlib import Path

from agent import FENCE, FIRST, LAST, LIVE, TITLE, transcript, turn_marker


class HostFixture:
    first_marker, last_marker, fence_marker, live_marker, title = FIRST, LAST, FENCE, LIVE, TITLE
    turn_marker = staticmethod(turn_marker)

    def __init__(self, binary, turns=20, sessions=1, native_load=False):
        if turns < 1 or sessions < 1:
            raise ValueError("turns and sessions must be positive")
        self.binary = Path(binary).resolve(strict=True)
        self.turns, self.sessions = turns, sessions
        self.native_load = native_load
        self.sequence = 0

    def command(self, *args):
        result = subprocess.run([self.binary, "host", *args], cwd=self.cwd, env=self.env,
                                stdin=subprocess.DEVNULL, capture_output=True,
                                timeout=30, text=True, check=False)
        if result.returncode:
            raise RuntimeError(f"host {args!r} exited {result.returncode}: {result.stderr}")
        return result.stdout.strip()

    def __enter__(self):
        self.temp = tempfile.TemporaryDirectory(prefix="nori-perf-")
        self.home = Path(self.temp.name).resolve()
        self.cwd = self.home / "project"
        self.cwd.mkdir()
        self.agent_log = self.home / "agent.jsonl"
        agent = Path(__file__).resolve().with_name("agent.py")
        self.env = {"HOME": str(self.home), "NORI_HOME": str(self.home),
                    "CODEX_HOME": str(self.home), "NORI_NO_ANALYTICS": "1",
                    "TERM": "xterm-256color", "LANG": "C.UTF-8",
                    "PATH": os.environ.get("PATH", os.defpath)}
        (self.home / "config.toml").write_text(
            f'agent = "synthetic"\n[projects.{json.dumps(str(self.cwd))}]\n'
            'trust_level = "trusted"\n'
            f'[tui]\nmax_sessions = {self.sessions + 2}\n'
            f'[host]\nmax_sessions = {self.sessions + 2}\n'
            '[[agents]]\nname = "Synthetic"\nslug = "synthetic"\n'
            f'[agents.distribution.local]\ncommand = {json.dumps(sys.executable)}\n'
            f'args = {json.dumps([str(agent), str(self.home)])}\n')
        (self.home / "turns").write_text(str(self.turns))
        if self.native_load:
            (self.home / "native-load").touch()
        self.started = False
        try:
            start = time.monotonic_ns()
            self.command("--port", "0")
            self.started = True
            self.host_start_ms = (time.monotonic_ns() - start) / 1e6
            # Creating the target first also makes it the first row in hosts
            # that retain insertion order. Search by title when timing larger lists.
            self.session_id = self.command("new", "--agent", "synthetic", "--prompt", TITLE)
            for index in range(1, self.sessions):
                self.command("new", "--agent", "synthetic", "--prompt", f"Filler sample {index:04d}")
            target_path = None
            deadline = time.monotonic() + 30
            while time.monotonic() < deadline:
                listing = self.command("ls")
                entries = self.agent_events()
                target_path = target_path or next((self.home / "transcripts").rglob(
                    f"{self.session_id}.jsonl"), None)
                # The host can briefly report ready between creation and dispatching
                # the first prompt. The persisted idle after the final tool is the fence.
                if target_path and listing.lower().count("ready") >= self.sessions:
                    data = target_path.read_bytes()
                    records = [json.loads(line) for line in data.split(b"\n")[:-1]]
                    events = [row["event"]["event"] for row in records if row["type"] == "session_event"]
                    if (any(event.get("update", {}).get("title") == FENCE for event in events)
                            and events[-1].get("update", {}).get("_meta", {}).get("nori", {}).get("status") == "idle"):
                        break
                time.sleep(.025)
            else:
                raise RuntimeError(f"Fixture failed to become ready: {listing}")
            counts_by_type = {}
            for event in events:
                update = event.get("update", {})
                if update.get("messageId", "").startswith(("user-", "answer-", "final-")) or update.get("toolCallId"):
                    kind = update["sessionUpdate"]
                    counts_by_type[kind] = counts_by_type.get(kind, 0) + 1
            records = [e for e in entries if e["event"] == "history_sent"]
            updates = list(transcript("synthetic", self.turns))
            if (len(records) != 1 or records[0]["updates"] != len(updates)
                    or sum(counts_by_type.values()) != len(updates)):
                raise ValueError("Synthetic agent sent an unexpected number of updates")
            self.counts = {"sessions": self.sessions, "sections": self.turns,
                           "acp_updates": len(updates), "update_types": counts_by_type,
                           "saved_session_events": len(events), "transcript_bytes": len(data),
                           "text_bytes": sum(
                               len(msg["params"]["update"].get("content", {}).get("text", "").encode())
                               for msg in updates if msg["params"]["update"]["sessionUpdate"]
                               in ("agent_message_chunk", "user_message_chunk"))}
            return self
        except BaseException:
            self.__exit__(None, None, None)
            raise

    def agent_events(self):
        if not self.agent_log.exists():
            return []
        return [json.loads(line) for line in self.agent_log.read_text().splitlines() if line]

    def live(self):
        return self.stream(1, 0)

    def stream(self, chunks=24, interval_ms=20):
        if chunks < 1 or interval_ms < 0:
            raise ValueError("invalid stream length or interval")
        self.sequence += 1
        request = self.home / "live.tmp"
        request.write_text(json.dumps([self.sequence, chunks, interval_ms]))
        request.replace(self.home / "live")
        return f"{LIVE}{self.sequence:04d}"

    def __exit__(self, *_):
        try:
            if self.started:
                self.command("stop")
        finally:
            self.temp.cleanup()
