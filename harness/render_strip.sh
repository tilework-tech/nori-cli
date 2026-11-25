#!/usr/bin/env bash
# Like render.sh, but strips the trailing newline from each capture first
# (otherwise Ghostty Web drops the top row). Usage: render_strip.sh DIR [cols rows]
set -euo pipefail
dir=$1; cols=${2:-120}; rows=${3:-36}
renderer=${GHOSTTY_RENDER:-/home/sprite/org/workspace/.claude/skills/tui-capture-with-ghostty-web/scripts/render-ansi.ts}
tmp=$(mktemp -d)
find "$dir" -name '*.ansi' | while read -r f; do
  [ -e "${f%.ansi}.png" ] && continue
  python3 -I -c 'import sys; open(sys.argv[2],"wb").write(open(sys.argv[1],"rb").read().rstrip(b"\n"))' "$f" "$tmp/in.ansi"
  timeout 120 bun "$renderer" --input "$tmp/in.ansi" --output "${f%.ansi}.png" --cols "$cols" --rows "$rows" >/dev/null
done
rm -rf "$tmp"
