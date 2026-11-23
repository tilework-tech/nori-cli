#!/usr/bin/env python3
"""Build the #730 c69d9d1c recheck sheet: the working-indicator row on three builds."""
import html, os, sys

OUT = sys.argv[1] + "/"
DEST = sys.argv[2]
BUILDS = ["54ae46d3", "920570ea", "c69d9d1c"]


def status_row(f):
    if not f or not os.path.exists(OUT + f):
        return None
    return [l for l in open(OUT + f).read().splitlines() if l.startswith("• ")][-1]


OK, BAD, KNOWN = "<span class=ok>{}</span>", "<span class=bad>{}</span>", "<span class=n>{}</span>"
ROWS = [
    ("Token-split inline **emphasis** (** is its own chunk)", 100, BAD.format("54ae46d3: bold word only; 920570ea: sentence cut") + "<br>" + OK.format("c69d9d1c: whole sentence"),
     ["r3-head-100/tokemph-1.txt", "r3-head2-100/tokemph-1.txt", "r4-head3-100/tokemph-1.txt"]),
    ("Token-split second **heading** in one block (Codex-style)", 100, BAD.format("920570ea: stuck on first heading") + "<br>" + OK.format("c69d9d1c: second heading"),
     ["r3-head-100/twohead-2.txt", "r3-head2-100/twohead-2.txt", "r4-head3-100/twohead-2.txt"]),
    ("Token-split first **heading**", 100, OK.format("ok"),
     [None, "r3-head2-100/tokhead-1.txt", "r4-head3-100/tokhead-1.txt"]),
    ("Gemini whole thought 2 (no separator)", 100, OK.format("ok"),
     [None, "r2-head2-100/gemini-2.txt", "r4-head3-100/gemini-2.txt"]),
    ("Inline **emphasis**, large chunks", 100, BAD.format("54ae46d3: bold word only") + "<br>" + OK.format("fixed since 920570ea"),
     ["head-100/inline-1.txt", "r3-head2-sweep/inline-1.txt", "r4-head3-100/inline-1.txt"]),
    ("Plain thought", 100, OK.format("ok"),
     ["head-100/plain-1.txt", "r3-head2-sweep/plain-1.txt", "r4-head3-100/plain-1.txt"]),
    ("Ordinary reply, no thoughts", 100, OK.format("ok"),
     ["head-100/reply-1.txt", "r3-head2-sweep/reply-1.txt", "r4-head3-100/reply-1.txt"]),
    ("CJK thought", 100, BAD.format("54ae46d3: timer clipped") + "<br>" + OK.format("timer and hint visible"),
     ["head-100/wide-1.txt", None, "r4-head3-100/wide-1.txt"]),
    ("CJK thought", 60, BAD.format("54ae46d3: timer clipped") + "<br>" + OK.format("timer and hint visible"),
     ["head-60/wide-1.txt", "r3-head2-60/wide-1.txt", "r4-head3-60/wide-1.txt"]),
    ("CJK + emoji thought", 100, BAD.format("54ae46d3: timer clipped") + "<br>" + OK.format("timer and hint visible"),
     ["r3-head-100x/emoji-1.txt", "r2-head2-100/emoji-1.txt", "r4-head3-100/emoji-1.txt"]),
    ("CJK + emoji thought", 60, BAD.format("54ae46d3: timer clipped") + "<br>" + OK.format("timer and hint visible"),
     ["r3-head-60/emoji-1.txt", "r3-head2-60/emoji-1.txt", "r4-head3-60/emoji-1.txt"]),
    ("CJK thought", 44, KNOWN.format("clips, as the PR documents"),
     [None, None, "r4-head3-44/wide-1.txt"]),
]

parts = ['''<!doctype html><html><head><meta charset="utf-8"><style>
body{background:#0f1110;color:#d7dbd8;font:14px/1.4 Helvetica,Arial,sans-serif;margin:16px;width:3020px}
h1{font-size:20px;margin:0 0 4px}.sub{color:#8a938d;margin-bottom:14px;max-width:2000px}
table{border-collapse:collapse}td,th{padding:8px;vertical-align:top;border-bottom:1px solid #222}th{text-align:left;color:#7fd99a}
td.cap{width:220px;color:#b9c2bc}.bad{color:#f08a7a}.ok{color:#7fd99a}
pre{background:#151716;border:1px solid #3a403c;margin:0;padding:6px 0;font:15px/1.35 "DejaVu Sans Mono","Noto Sans Mono CJK JP",monospace;color:#d7dbd8;overflow:hidden;white-space:pre}
.n{color:#8a938d;font-size:12px;margin-top:3px}.miss{color:#5b625e;font-style:italic}</style></head><body>
<h1>nori-cli #730 recheck at c69d9d1c: working-indicator row on three builds</h1>
<div class="sub">The real TUI in tmux, driven by a synthetic ACP agent that pauses at each step. Same agent, prompts and width for every build.
54ae46d3 = first PR commit, 920570ea = previous head, c69d9d1c = current head. Each box is the exact terminal width, filled from the tmux capture-pane text.</div>
<table><tr><th></th>''' + "".join(f"<th>{b}</th>" for b in BUILDS) + "</tr>"]
for cap, width, verdict, files in ROWS:
    cells = []
    for f in files:
        row = status_row(f)
        if row is None:
            cells.append('<td><div class=miss>not captured</div></td>')
        else:
            cells.append(f'<td><pre style="width:{width * 9.04:.0f}px">{html.escape(row)}</pre>'
                         f'<div class=n>{width} cols · {f}</div></td>')
    parts.append(f'<tr><td class=cap>{cap}, {width} cols<br>{verdict}</td>{"".join(cells)}</tr>')
parts.append('</table></body></html>')
open(DEST, "w").write("\n".join(parts))
