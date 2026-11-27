"""Render one tmux `capture-pane -e` screen (.ansi) to a still GIF with agg.

usage: python3 -I ansi2gif.py SCREEN.ansi OUT.gif [cols rows]
Needs AGG (path to agg) and FONT_DIR in the environment.
"""
import json
import os
import subprocess
import sys
import tempfile

src, out = sys.argv[1], sys.argv[2]
cols, rows = (int(sys.argv[3]), int(sys.argv[4])) if len(sys.argv) > 4 else (110, 40)
text = open(src, encoding="utf-8", errors="replace").read().rstrip("\n")
frame = "\x1b[H\x1b[2J" + text.replace("\n", "\r\n")
with tempfile.NamedTemporaryFile("w", suffix=".cast", delete=False) as cast:
    cast.write(json.dumps({"version": 2, "width": cols, "height": rows,
                           "theme": {"fg": "#d4d4d4", "bg": "#141414",
                                     "palette": ":".join(["#141414", "#e06c75", "#5fd068", "#e5c07b", "#61afef", "#c678dd", "#56b6c2", "#d4d4d4", "#7f848e", "#ef8a92", "#7ee787", "#f0d58a", "#82c4ff", "#d7a1ea", "#7fd1dc", "#ffffff"])}}) + "\n")
    cast.write(json.dumps([0.0, "o", frame]) + "\n")
    cast.write(json.dumps([1.0, "o", ""]) + "\n")
subprocess.run([os.environ["AGG"], "--font-dir", os.environ["FONT_DIR"],
                "--font-family", "JetBrains Mono,Noto Sans Mono CJK JP", "--font-size", "14",
                "--last-frame-duration", "1", cast.name, out], check=True, capture_output=True)
os.unlink(cast.name)
