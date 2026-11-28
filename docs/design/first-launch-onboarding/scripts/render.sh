#!/usr/bin/env bash
# Render a cast to GIF, or a still PNG at time T (seconds).
#   render.sh gif  in.cast out.gif
#   render.sh still in.cast out.png T
set -euo pipefail
THEME="0e0e0e,dde1e6,161616,ee5396,42be65,f1c21b,78a9ff,be95ff,3ddbd9,dde1e6,525252,ff7eb6,6fdc8c,fddc69,a6c8ff,d4bbff,82cfff,f2f4f8"
FONTS="JetBrains Mono,DejaVu Sans Mono,DejaVu Sans,Noto Sans Mono CJK JP"
mode=$1 in=$2 out=$3
if [ "$mode" = gif ]; then
  agg --theme "$THEME" --font-family "$FONTS" --font-size 20 -q --idle-time-limit 2 --last-frame-duration 2 "$in" "$out"
else
  T=$4 tmp=$(mktemp -d)
  python3 - "$in" "$T" > "$tmp/cut.cast" <<'PY'
import json, sys
lines = open(sys.argv[1]).read().splitlines()
t = float(sys.argv[2]); print(lines[0])
for l in lines[1:]:
    if json.loads(l)[0] <= t: print(l)
print(json.dumps([t + 0.1, "o", ""]))
PY
  agg -q --theme "$THEME" --font-family "$FONTS" --font-size 20 --last-frame-duration 0.1 "$tmp/cut.cast" "$tmp/x.gif"
  convert "$tmp/x.gif" -coalesce "$tmp/f-%04d.png"
  cp "$(ls "$tmp"/f-*.png | sort | tail -1)" "$out"
  rm -rf "$tmp"
fi
