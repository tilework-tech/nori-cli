#!/usr/bin/env python3
"""Resume a failed host row with Ctrl+R, then open it and prompt its replacement."""

import argparse
from contextlib import ExitStack
import hashlib
import json
import os
from pathlib import Path
import platform
import signal
import tempfile
import time

from fixture import HostFixture
from replay import CLOCK, SECTION, Terminal, elapsed
from transport import TransportProbe


def wait_for(probe, seconds=10):
    end = time.monotonic() + seconds
    while time.monotonic() < end:
        result = probe()
        if result:
            return result
        time.sleep(.005)
    raise TimeoutError("Failed to reach a recovery milestone")


def sample(binary, turns, native_load, wrapper, audit=False):
    with ExitStack() as resources:
        f = resources.enter_context(HostFixture(binary, turns, native_load=native_load))
        pid = next(e["pid"] for e in f.agent_events() if e["event"] == "history_sent")
        # Only terminate the fixture-owned adapter. The same host keeps the failed row.
        os.kill(pid, signal.SIGTERM)
        wait_for(lambda: "failed" in f.command("ls"))
        wire = resources.enter_context(TransportProbe(f.home, f.session_id))
        root = Path(resources.enter_context(tempfile.TemporaryDirectory(prefix="nori-recover-")))
        term = Terminal(root, wrapper, f, [str(binary), "attach", *wire.attach_args])
        resources.callback(term.close)
        term.launch()
        term.wait(lambda s: f.title in s and "1 failed" in s)
        assert term.call("display-message", "-p", "-t", term.name, "#{alternate_on}").strip() == "1"
        start = CLOCK()
        term.send("C-r")
        picker = term.wait(lambda s: "1 ready" in s and f.title in s)
        result = {"ctrl_r_to_ready_row_ms": elapsed(start), "rows": 1,
                  "ctrl_r_stayed_in_picker": "enter open" in picker}
        assert result["ctrl_r_stayed_in_picker"]
        enter = CLOCK()
        term.send("Enter")
        seen = {}

        def observe(screen):
            if "enter open" in screen or f.title not in screen.splitlines()[1]:
                return False
            body = "\n".join(screen.splitlines()[3:-4])
            for key, found in [("content", f.first_marker in body or SECTION.search(body)),
                               ("latest", f.last_marker in body), ("tool", f.fence_marker in body)]:
                if found:
                    seen.setdefault("enter_to_first_" + key + "_ms", elapsed(enter))
            return len(seen) == 3

        term.wait(observe)
        result.update(seen)
        # Unlike the previous reply/tool, which can occur in both journal passes,
        # only the replacement adapter can acknowledge this post-resume prompt.
        prompt = "RecoveryProbe"
        term.send(prompt, literal=True)
        term.wait(lambda s: any(prompt in line for line in s.splitlines()[-4:]))
        # A recent paste burst deliberately holds the first Enter until its flush.
        time.sleep(.12)
        term.send("Enter")
        term.wait(lambda s: "Synthetic acknowledgement." in s)
        result["enter_to_new_reply_ms"] = elapsed(enter)
        if audit:
            # Walk one screen at a time toward the first event. Overlapping pages
            # keep the largest visible index nonincreasing, including across resets.
            previous_max = turns + 1
            seen_sections = set()
            page = term.settled()
            for _ in range(4 * turns + 30):
                numbers = [int(i) for i in SECTION.findall(page)]
                if numbers:
                    assert numbers == sorted(set(numbers)) and max(numbers) <= previous_max
                    previous_max = max(numbers)
                    seen_sections.update(numbers)
                if f.first_marker in page:
                    break
                before = page
                term.send("PageUp")
                term.wait(lambda s: before != s)
                page = term.settled()
            assert f.first_marker in page and seen_sections == set(range(1, turns + 1))
            result["audited_sections"] = len(seen_sections)
        term.send("Left")
        term.wait(lambda s: f.title in s and "enter open" in s)
        term.send("Enter")
        row = wait_for(lambda: wire.snapshot() if len(wire.snapshot()["subscriptions"]) >= 2 else None)
        if wire.error:
            raise RuntimeError("Transport probe failed") from wire.error
        applied = row["subscriptions"][-1]["cursor"]
        # Ready can arrive before the host has appended the final ReplayFinished
        # and idle to its journal. Confirm the cursor also reaches every envelope
        # delivered up to re-selection, including the replacement's fresh reply.
        assert not row["gaps"] and applied >= row["subscriptions"][-1]["delivered_seq"]
        assert applied >= row["history_watermark"] > 0
        assert row["replay_finished_ns"] and row["history_through_ns"]
        result.update(history_watermark=row["history_watermark"], applied_after_probe=applied,
                      enter_to_wire_watermark_ms=round((row["history_through_ns"] - enter) / 1e6, 3),
                      enter_to_replay_finished_wire_ms=round((row["replay_finished_ns"] - enter) / 1e6, 3),
                      enter_to_confirmed_cursor_ms=round((row["subscriptions"][-1]["at_ns"] - enter) / 1e6, 3),
                      event_gaps=row["gaps"])
        ledger = [e for e in f.agent_events() if e["pid"] != pid]
        new = next(e["pid"] for e in ledger if e["method"] == "initialize")
        assert any(prompt in e.get("text", "") and e["pid"] == new for e in ledger), [e.get("text", "")[-100:] for e in ledger]
        assert f.session_id in f.command("ls")
        for e in ledger:
            if e["event"] == "response" and e["method"] in ("initialize", "session/new", "session/load"):
                result["ctrl_r_to_agent_" + e["method"].replace("/", "_") + "_ms"] = round(
                    (e["monotonic_ns"] - start) / 1e6, 3)
        result["fixture"] = f.counts
        return result


def restart(binary, wrapper):
    """Check picker inventory after a clean host restart in the same synthetic home."""
    with ExitStack() as resources:
        f = resources.enter_context(HostFixture(binary, 4))
        saved = next((f.home / "transcripts").rglob(f"{f.session_id}.jsonl"))
        f.command("stop")
        f.command("--port", "0")
        listing = json.loads(f.command("ls", "--json"))["sessions"]
        directory = Path(resources.enter_context(tempfile.TemporaryDirectory(prefix="nori-restart-")))
        term = Terminal(directory, wrapper, f, [str(binary), "attach"])
        resources.callback(term.close)
        term.launch()
        screen = term.wait(lambda s: "Sessions 0" in s and "No sessions yet" in s)
        assert saved.is_file() and f.fence_marker in saved.read_text()
        assert not listing and f.title not in screen
        return {"saved_transcript_contains_fence": True, "host_ls_rows_after_restart": len(listing),
                "attach_picker_rows_after_restart": 0}


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("binary", type=Path)
    p.add_argument("--turns", nargs="+", type=int, default=[4, 64])
    p.add_argument("--samples", type=int, default=3)
    p.add_argument("--modes", nargs="+", choices=["transcript", "native"], default=["transcript", "native"])
    p.add_argument("--tmux-wrapper", default=os.environ.get("NORI_BENCH_TMUX"))
    p.add_argument("--output", type=Path)
    p.add_argument("--check-restart", action="store_true")
    args = p.parse_args()
    binary = args.binary.resolve()
    result = {"platform": platform.platform(), "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
              "terminal": "120x40 tmux loopback via diagnostic wire probe", "samples": []}
    if args.check_restart:
        result["restart"] = restart(binary, args.tmux_wrapper)
        print(json.dumps(result), flush=True)
        if args.output:
            args.output.write_text(json.dumps(result, indent=2) + "\n")
        return
    for length in args.turns:
        for mode in args.modes:
            for index in range(args.samples):
                sample_data = sample(binary, length, mode == "native", args.tmux_wrapper, audit=index == 0)
                result["samples"].append({"sections": length, "mode": mode, **sample_data})
                print(json.dumps(result["samples"][-1]), flush=True)
                if args.output:
                    args.output.write_text(json.dumps(result, indent=2) + "\n")


if __name__ == "__main__":
    main()
