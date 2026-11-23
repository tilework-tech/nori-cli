#!/usr/bin/env python3
"""Build the before/after proof sheet for the #730 recheck from tmux text captures."""
import html
import sys

OUT = sys.argv[1] + "/"
DEST = sys.argv[2]


def status_row(f):
    return [l for l in open(OUT + f).read().splitlines() if l.startswith("• ")][-1]


ROWS = [
    ("CJK + emoji thought, 100 cols<br><span class=ok>fixed</span>", 100,
     "r3-head-100x/emoji-1.txt", "r2-head2-100/emoji-1.txt"),
    ("CJK + emoji thought, 60 cols<br><span class=ok>fixed</span>", 60,
     "r3-head-60/emoji-1.txt", "r3-head2-60/emoji-1.txt"),
    ("CJK thought, 60 cols<br><span class=ok>fixed</span>", 60,
     "head-60/wide-1.txt", "r3-head2-60/wide-1.txt"),
    ("Inline **emphasis** in a sentence sent in large chunks, 100 cols<br><span class=ok>fixed</span>", 100,
     "head-100/inline-1.txt", "r3-head2-sweep/inline-1.txt"),
    ("NEW REGRESSION: inline **emphasis** streamed token by token (** arrives as its own chunk), 100 cols"
     "<br><span class=bad>sentence cut where a newline was inserted</span>", 100,
     "r3-head-100/tokemph-1.txt", "r3-head2-100/tokemph-1.txt"),
    ("NEW REGRESSION: second token-streamed **heading** in one thought block (Codex-style), 100 cols"
     "<br><span class=bad>stuck on the first heading</span>", 100,
     "r3-head-100/twohead-2.txt", "r3-head2-100/twohead-2.txt"),
]

parts = ['''<!doctype html><html><head><meta charset="utf-8"><style>
body{background:#0f1110;color:#d7dbd8;font:14px/1.4 Helvetica,Arial,sans-serif;margin:16px;width:2150px}
h1{font-size:20px;margin:0 0 4px}.sub{color:#8a938d;margin-bottom:14px;max-width:1900px}
table{border-collapse:collapse}td,th{padding:8px;vertical-align:top;border-bottom:1px solid #222}th{text-align:left;color:#7fd99a}
td.cap{width:270px;color:#b9c2bc}.bad{color:#f08a7a}.ok{color:#7fd99a}
pre{background:#151716;border:1px solid #3a403c;margin:0;padding:6px 0;font:15px/1.35 "DejaVu Sans Mono","Noto Sans Mono CJK JP",monospace;color:#d7dbd8;overflow:hidden;white-space:pre}
.n{color:#8a938d;font-size:12px;margin-top:3px}</style></head><body>
<h1>nori-cli #730 recheck: working-indicator row before and after the fix commit</h1>
<div class="sub">The real TUI, driven in tmux by a synthetic ACP agent that pauses at each step. Both builds got the same agent, prompts, and terminal width.
Before = 54ae46d3 (previous head). After = 920570ea. Each box is the exact terminal width, filled from the tmux capture-pane text.</div>
<table><tr><th></th><th>Before (54ae46d3)</th><th>After (920570ea)</th></tr>''']
for cap, width, before, after in ROWS:
    cells = "".join(
        f'<td><pre style="width:{width * 9.04:.0f}px">{html.escape(status_row(f))}</pre>'
        f'<div class=n>{width} cols · {f}</div></td>'
        for f in (before, after))
    parts.append(f'<tr><td class=cap>{cap}</td>{cells}</tr>')
parts.append('</table></body></html>')
open(DEST, "w").write("\n".join(parts))
