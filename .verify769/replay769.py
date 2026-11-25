#!/usr/bin/env python3
"""Independent attach replay verification for nori-cli #769.

For each sample: fresh home, real host, kiln-seeded saved history, `nori attach`
in a draining tmux pane. Measures selection to first content / latest reply /
final tool / live reply / handled draft, typing during a paced stream, PageUp,
and then reconstructs the entire scrollback by stitching pages one PageUp
(10 rows) apart, so duplicated, missing or reordered rows are detected, not
only the set of section markers.

Modes:
  open     open a ready saved row (default)
  recover  kill the row's adapter, Ctrl+R in the picker, open, prompt the replacement
"""
import argparse
import hashlib
import json
import os
import platform
import re
import signal
import statistics
import sys
import tempfile
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from kiln import NS, SYNC_ON, KilnHost, Pane, ms_since  # noqa: E402

MARK = re.compile(r"KilnMark(\d{4})")
NOTE = re.compile(r"Closing note (\d{3}) for the run\.")
PAGE = 10


def split(screen):
    lines = [l.rstrip() for l in screen.splitlines()]
    comp = max(i for i, l in enumerate(lines) if l.startswith("› ") or l == "›")
    assert lines[comp - 1] == "" and lines[2] == "", "layout spacer rows changed"
    return lines[3:comp - 1], lines[comp]


def history_text(screen):
    return "\n".join(split(screen)[0])


def wait_for_list(pane, rows):
    return pane.wait(lambda s: f"{rows} ready" in s and "enter open" in s)


def wait_failed_list(pane):
    return pane.wait(lambda s: "1 failed" in s and "enter open" in s)


def stitch(pane, max_pages):
    """Return the full history rows (oldest first) and PageUp latencies."""
    page = pane.settle(0.15)
    rows, _ = split(page)
    full = list(rows)
    lat = []
    for _ in range(max_pages):
        before = history_text(page)
        t = NS()
        pane.key("PageUp")
        try:
            pane.wait(lambda s: history_text(s) != before, timeout=1.5)
            lat.append(ms_since(t))
        except TimeoutError:
            return full, lat, True
        page = pane.settle(0.12)
        b, _ = split(page)
        h = len(b)
        for s in range(0, PAGE + 1):
            if b[s:] == full[:h - s]:
                break
        else:
            raise AssertionError("page does not overlap previous page by a scroll step:\n"
                                 + "\n".join(b) + "\n---\n" + "\n".join(full[:h]))
        if s == 0:
            return full, lat, True
        if s != PAGE and "KilnOpening" not in "\n".join(b):
            raise AssertionError(f"scroll moved {s} rows before reaching the start")
        full = b[:s] + full
        if s != PAGE:
            return full, lat, True
    return full, lat, False


def audit(full, sections, lives, recovered_prompt=None):
    text = "\n".join(full)
    order = []
    for line in full:
        for m in MARK.finditer(line):
            order.append(("mark", int(m.group(1))))
        for m in NOTE.finditer(line):
            order.append(("note", int(m.group(1))))
        for tok in ("KilnOpening", "KilnLatestEnd", "final.txt", "Final summary across"):
            if tok in line:
                order.append((tok, 0))
        for n in range(1, lives + 1):
            if f"KilnLive{n:04d}" in line:
                order.append(("live", n))
        if recovered_prompt and f"Kiln answer to {recovered_prompt}." in line:
            order.append(("answer", 0))
    expect = ([("KilnOpening", 0)] + [("mark", i) for i in range(1, sections + 1)]
              + [("Final summary across", 0)] + [("note", n) for n in range(1, 91)]
              + [("KilnLatestEnd", 0), ("final.txt", 0)]
              + [("live", n) for n in range(1, lives + 1)])
    if recovered_prompt:
        expect.append(("answer", 0))
    counts = {}
    for item in order:
        counts[item] = counts.get(item, 0) + 1
    dups = sorted(f"{k[0]}:{k[1]}" for k, v in counts.items() if v > 1)
    missing = sorted(f"{k[0]}:{k[1]}" for k in expect if k not in counts)
    return {"ordered_exactly_once": order == expect, "duplicates": dups[:20],
            "duplicate_count": len(dups), "missing": missing[:20], "rows": len(full),
            "sha256": hashlib.sha256(text.encode()).hexdigest()}


def open_sample(binary, args, workdir):
    out = {}
    with KilnHost(binary, args.sections, native_load=args.native,
                  extra_rows=args.extra_rows) as host:
        pane = Pane(workdir, host, [host.nori, "attach"], args.cols, args.rows)
        try:
            if args.mode == "recover":
                pid = next(e["pid"] for e in host.ledger() if e["event"] == "seeded")
                os.kill(pid, signal.SIGTERM)
                deadline = time.monotonic() + 15
                while "failed" not in host.run("ls"):
                    if time.monotonic() > deadline:
                        raise TimeoutError("row never failed")
                    time.sleep(0.02)
            t0 = pane.start()
            if args.mode == "recover":
                wait_failed_list(pane)
            else:
                wait_for_list(pane, 1 + args.extra_rows)
            out["launch_to_list_ms"] = ms_since(t0)
            if args.extra_rows:
                t = NS()
                pane.key("/")
                pane.text("kiln seed")
                pane.wait(lambda s: "kiln seed history" in s and "filler row" not in s)
                out["filter_to_single_row_ms"] = ms_since(t)
            if args.mode == "recover":
                t = NS()
                pane.key("C-r")
                pane.wait(lambda s: "1 ready" in s and "enter open" in s)
                out["ctrl_r_to_ready_row_ms"] = ms_since(t)
            offset = pane.raw.stat().st_size
            seen = {}
            t_enter = NS()
            pane.key("Enter")

            def has_content(s):
                if "enter open" in s:
                    return False
                h = history_text(s)
                return bool(MARK.search(h) or NOTE.search(h) or "KilnOpening" in h
                            or "KilnLatestEnd" in h)
            pane.wait(has_content)
            seen["enter_to_first_content_ms"] = ms_since(t_enter)
            live1 = host.live(1) if args.mode == "open" else None
            pane.text("ж1")
            t_draft = NS()
            targets = {"enter_to_latest_reply_ms": "KilnLatestEnd",
                       "enter_to_final_tool_ms": "final.txt"}
            if live1:
                targets["enter_to_live_reply_ms"] = live1

            def check(s):
                h, comp = split(s)
                body = "\n".join(h)
                for k, tok in targets.items():
                    if k not in seen and tok in body:
                        seen[k] = ms_since(t_enter)
                if "draft_ms" not in seen and "ж1" in comp:
                    seen["draft_ms"] = ms_since(t_draft)
                return len(seen) == len(targets) + 2
            pane.wait(check)
            out.update(seen)
            raw = pane.raw.read_bytes()[offset:]
            out["frames_to_catchup"] = raw.count(SYNC_ON)
            out["bytes_to_catchup"] = len(raw)
            prompt = None
            lives = 1 if live1 else 0
            if args.mode == "recover":
                # clear the draft; send a prompt only the replacement adapter can answer
                for _ in range(2):
                    pane.key("BSpace")
                prompt = f"recovered probe {os.getpid()}"
                pane.text(prompt)
                pane.wait(lambda s: prompt in split(s)[1])
                time.sleep(0.15)
                pane.key("Enter")
                try:
                    pane.wait(lambda s: f"Kiln answer to {prompt}." in history_text(s), timeout=15)
                    out["enter_to_new_answer_ms"] = ms_since(t_enter)
                    out["new_answer_seen"] = True
                except TimeoutError as lost:
                    # e.g. the connection dropped while the prompt was in flight
                    out["new_answer_seen"] = False
                    out["new_answer_failure_screen"] = str(lost)
                    prompt = None
                ledger = host.ledger()
                old = next(e["pid"] for e in ledger if e["event"] == "seeded")
                out["answered_by_replacement"] = prompt is not None and any(
                    e["event"] == "prompt" and e["text"] == prompt and e["pid"] != old for e in ledger)
                out["replacement_ledger_ms_from_enter"] = [
                    [e["event"], e.get("method", ""), round((e["monotonic_ns"] - t_enter) / 1e6, 1)]
                    for e in ledger if e["pid"] != old]
                pane.text("ж1")
            else:
                pane.settle(0.2)
                t = NS()
                live2 = host.live(2, chunks=30, interval_ms=15)
                pane.wait(lambda s: "tick2 " in history_text(s))
                tk = NS()
                pane.text("ж2")
                pane.wait(lambda s: "ж1ж2" in split(s)[1])
                out["key_during_stream_ms"] = ms_since(tk)
                pane.wait(lambda s: live2 in history_text(s))
                out["stream_complete_ms"] = ms_since(t)
                lives = 2
            final = pane.settle(0.3)
            h, comp = split(final)
            t = NS()
            pane.text("q")
            pane.wait(lambda s: "q" in split(s)[1][-3:])
            out["settled_key_ms"] = ms_since(t)
            final = pane.settle(0.2)
            h, comp = split(final)
            want = "› ж1ж2q" if args.mode == "open" else "› ж1q"
            out["draft_line"] = comp
            out["draft_intact"] = comp == want
            out["final_history_sha256"] = hashlib.sha256("\n".join(h).encode()).hexdigest()
            out["final_screen"] = final
            out["final_ansi"] = pane.ansi()
            try:
                full, lat, top = stitch(pane, args.sections * 30 + 60)
                stitch_error = None
            except AssertionError as broken:
                # A notice pinned below the history (e.g. a lost prompt) breaks paging;
                # record the sample as failing instead of aborting the run.
                full, lat, top, stitch_error = [], [], False, str(broken)[:4000]
            out["reached_start"] = top
            out["page_up_first_ms"] = lat[0] if lat else None
            out["page_up_ms_median"] = round(statistics.median(lat), 2) if lat else None
            out["page_up_ms_max"] = max(lat) if lat else None
            out["page_up_ms_last5_median"] = round(statistics.median(lat[-5:]), 2) if lat else None
            out["page_ups"] = len(lat)
            out["audit"] = audit(full, args.sections, lives, prompt) if stitch_error is None else {
                "ordered_exactly_once": False, "stitch_error": stitch_error}
            plain = re.sub(rb"\x1b\[[0-9;?]*[A-Za-z]", b"", pane.raw.read_bytes())
            out["disconnect_notices"] = plain.count(b"Connection lost")
            out["restored_notices"] = plain.count(b"Connection restored")
            out["scrollback_disconnect_rows"] = sum("Connection lost" in l or "Connection restored" in l for l in full)
            out["fixture"] = {"sections": args.sections, "extra_rows": args.extra_rows,
                              "native_load": args.native, "geometry": f"{args.cols}x{args.rows}"}
            if args.keep_scrollback:
                out["scrollback"] = full
            return out
        finally:
            pane.close()


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--build", nargs=2, action="append", metavar=("LABEL", "BINARY"), required=True)
    p.add_argument("--mode", choices=["open", "recover"], default="open")
    p.add_argument("--sections", type=int, default=64)
    p.add_argument("--extra-rows", type=int, default=0)
    p.add_argument("--native", action="store_true")
    p.add_argument("--samples", type=int, default=3)
    p.add_argument("--cols", type=int, default=100)
    p.add_argument("--rows", type=int, default=30)
    p.add_argument("--keep-scrollback", action="store_true")
    p.add_argument("--out", type=Path, required=True)
    args = p.parse_args()
    builds = [(label, Path(b).resolve()) for label, b in args.build]
    result = {"platform": platform.platform(), "args": {k: str(v) for k, v in vars(args).items()},
              "builds": {l: hashlib.sha256(b.read_bytes()).hexdigest() for l, b in builds},
              "samples": []}
    for i in range(args.samples):
        order = builds if i % 2 == 0 else builds[::-1]
        for label, binary in order:
            with tempfile.TemporaryDirectory(prefix=".kiln-tty-") as d:
                row = open_sample(binary, args, Path(d))
            row.update(build=label, index=i)
            result["samples"].append(row)
            brief = {k: v for k, v in row.items() if not k.startswith(("final_", "scrollback"))}
            print(json.dumps(brief), flush=True)
            args.out.write_text(json.dumps(result, indent=1) + "\n")


if __name__ == "__main__":
    main()
