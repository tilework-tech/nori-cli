#!/usr/bin/env python3
"""Summarize replay769 JSON files: median (min-max) per build, plus correctness checks."""
import json
import statistics
import sys
from pathlib import Path

KEYS = ["launch_to_list_ms", "filter_to_single_row_ms", "ctrl_r_to_ready_row_ms",
        "enter_to_first_content_ms", "draft_ms", "enter_to_latest_reply_ms",
        "enter_to_final_tool_ms", "enter_to_live_reply_ms", "enter_to_new_answer_ms",
        "key_during_stream_ms", "settled_key_ms", "page_up_first_ms",
        "page_up_ms_median", "page_up_ms_max", "page_up_ms_last5_median",
        "frames_to_catchup", "bytes_to_catchup"]


def cell(values):
    values = [v for v in values if v is not None]
    if not values:
        return "—"
    med = statistics.median(values)
    fmt = (lambda v: f"{v:,.0f}") if med >= 100 else (lambda v: f"{v:.1f}")
    return f"{fmt(med)} ({fmt(min(values))}–{fmt(max(values))})"


def summarize(path):
    data = json.loads(Path(path).read_text())
    builds = list(data["builds"])
    rows = {b: [s for s in data["samples"] if s["build"] == b] for b in builds}
    out = [f"### {Path(path).stem}  ({data['args']['mode']}, {data['args']['sections']} sections, "
           f"{data['args']['cols']}x{data['args']['rows']}, extra rows {data['args']['extra_rows']}, "
           f"native load {data['args']['native']}; n={', '.join(f'{b} {len(rows[b])}' for b in builds)})",
           "", "| metric | " + " | ".join(builds) + " |", "|---|" + "---:|" * len(builds)]
    for k in KEYS:
        if any(s.get(k) is not None for b in builds for s in rows[b]):
            out.append(f"| {k} | " + " | ".join(cell([s.get(k) for s in rows[b]]) for b in builds) + " |")
    checks = []
    for b in builds:
        ok = all(s["audit"]["ordered_exactly_once"] for s in rows[b])
        dup = sum(s["audit"].get("duplicate_count", 0) for s in rows[b])
        broken = sum(1 for s in rows[b] if "stitch_error" in s["audit"])
        drafts = all(s["draft_intact"] for s in rows[b])
        start = all(s["reached_start"] for s in rows[b])
        hashes = sorted({s["audit"]["sha256"][:12] for s in rows[b] if "sha256" in s["audit"]})
        finals = sorted({s["final_history_sha256"][:12] for s in rows[b]})
        extra = ""
        if rows[b] and "answered_by_replacement" in rows[b][0]:
            extra = (f", new prompt answered by replacement {sum(1 for s in rows[b] if s['answered_by_replacement'])}/{len(rows[b])}"
                     f", scrollback unreadable (pinned notice) {broken}")
        if rows[b] and "disconnect_notices" in rows[b][0]:
            extra += (f", samples with a disconnect {sum(1 for s in rows[b] if s['disconnect_notices'] or s['restored_notices'])}/{len(rows[b])}"
                      f", samples with a disconnect row left in scrollback {sum(1 for s in rows[b] if s['scrollback_disconnect_rows'])}")
        checks.append(f"- {b}: scrollback ordered exactly once {ok} (duplicates {dup}), reached start "
                      f"{start}, draft intact {drafts}{extra}; scrollback sha {hashes}; final screen sha {finals}")
    out += ["", *checks, ""]
    return "\n".join(out)


if __name__ == "__main__":
    print("\n".join(summarize(p) for p in sys.argv[1:]))
