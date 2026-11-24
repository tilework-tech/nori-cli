#!/usr/bin/env bash
# Render every .ansi capture under a results directory to PNG with the
# pinned Ghostty Web renderer. Usage: harness/render.sh results/<sha8> [cols rows]
set -euo pipefail
dir=$1; cols=${2:-120}; rows=${3:-36}
renderer=${GHOSTTY_RENDER:-/home/sprite/org/workspace/.claude/skills/tui-capture-with-ghostty-web/scripts/render-ansi.ts}
find "$dir" -name '*.ansi' | while read -r f; do
  bun "$renderer" --input "$f" --output "${f%.ansi}.png" --cols "$cols" --rows "$rows" >/dev/null
done
