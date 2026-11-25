#!/usr/bin/env python3
"""Labelled failed-session recovery stress for nori-cli #769.

Each attempt runs one full `replay769.py --mode recover` sample (kill the
adapter, Ctrl+R, open, prompt the replacement, stitch and audit the whole
scrollback) and labels it. Builds alternate per attempt and the restoration
load alternates between transcript fallback and native load every two
attempts, so every build sees the same machine load. Run several loops in
parallel to add scheduling pressure.

Labels (conclusive): pass, notice_in_history, history_mismatch, prompt_lost,
replay_unavailable. Inconclusive: unsent_draft (the probe stayed in the
composer with no notice), error (harness exception).
"""
import argparse
import json
import sys
import tempfile
import traceback
from pathlib import Path
from types import SimpleNamespace

sys.path.insert(0, str(Path(__file__).resolve().parent))
from replay769 import open_sample  # noqa: E402


def label(row):
    screen = row.get("final_screen", "") + row.get("new_answer_failure_screen", "")
    if "Replay unavailable" in screen:
        return "replay_unavailable"
    if row.get("disconnect_notices") or row.get("scrollback_disconnect_rows"):
        return "notice_in_history"
    if not row.get("new_answer_seen"):
        failure = row.get("new_answer_failure_screen", "")
        if "recovered probe" in failure.split("›")[-1]:
            return "unsent_draft"
        return "prompt_lost"
    if not row.get("answered_by_replacement"):
        return "prompt_lost"
    if not row["audit"].get("ordered_exactly_once") or not row.get("draft_intact"):
        return "history_mismatch"
    return "pass"


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--build", nargs=2, action="append", metavar=("LABEL", "BINARY"), required=True)
    p.add_argument("--attempts", type=int, required=True, help="attempts per build")
    p.add_argument("--sections", type=int, default=64)
    p.add_argument("--tag", required=True)
    p.add_argument("--out", type=Path, required=True, help="JSONL, one line per attempt")
    p.add_argument("--failures", type=Path, required=True, help="directory for failing screens")
    a = p.parse_args()
    a.failures.mkdir(parents=True, exist_ok=True)
    builds = [(l, Path(b).resolve()) for l, b in a.build]
    with a.out.open("a") as out:
        for i in range(a.attempts):
            native = (i // 2) % 2 == 1
            order = builds if i % 2 == 0 else builds[::-1]
            for name, binary in order:
                args = SimpleNamespace(mode="recover", sections=a.sections, native=native,
                                       extra_rows=0, cols=100, rows=30, keep_scrollback=True)
                try:
                    with tempfile.TemporaryDirectory(prefix=".kiln-tty-") as d:
                        row = open_sample(binary, args, Path(d))
                    outcome = label(row)
                except Exception as e:  # inconclusive harness failure; keep going
                    row, outcome = {"error": "".join(traceback.format_exception_only(e))[-2000:]}, "error"
                brief = {"tag": a.tag, "attempt": i, "build": name, "native": native,
                         "sections": a.sections, "outcome": outcome,
                         "enter_to_latest_reply_ms": row.get("enter_to_latest_reply_ms"),
                         "enter_to_new_answer_ms": row.get("enter_to_new_answer_ms"),
                         "disconnect_notices": row.get("disconnect_notices"),
                         "audit": {k: v for k, v in row.get("audit", {}).items() if k != "sha256"},
                         "error": row.get("error")}
                out.write(json.dumps(brief) + "\n")
                out.flush()
                if outcome != "pass":
                    (a.failures / f"{a.tag}-{i}-{name}.json").write_text(json.dumps(row, indent=1))


if __name__ == "__main__":
    main()
