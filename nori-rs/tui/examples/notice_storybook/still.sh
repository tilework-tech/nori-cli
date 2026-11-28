# Capture one storybook frame as GIF and PNG through asciinema and agg, which
# render the ▎ rail and the 〉 prompt that Ghostty Web leaves blank.
#
# Usage: bash still.sh <storybook-binary> <out-dir> <name> <cols> <rows> <expect> -- <storybook args>
set -euo pipefail
BIN=$1 OUT=$2 NAME=$3 COLS=$4 ROWS=$5 EXPECT=$6
shift 7
TUI=${TUI:?set TUI to the tui-puppeteering-with-tmux directory}
SESSION=notice-still
mkdir -p "$OUT"
"$TUI/tui-start" "$SESSION" "sleep 0.3; asciinema rec --overwrite -q --window-size ${COLS}x${ROWS} -c '$BIN $*' $OUT/$NAME.cast" >/dev/null
"$TUI/tmux-isolated" resize-window -t "$SESSION" -x "$COLS" -y "$ROWS"
"$TUI/tui-assert" "$SESSION" "$EXPECT" 10
sleep 0.5
"$TUI/tui-capture" "$SESSION" > "$OUT/$NAME.txt"
"$TUI/tui-send" "$SESSION" --keys C-c
"$TUI/tui-send" "$SESSION" q
sleep 1
"$TUI/tui-stop" "$SESSION" >/dev/null
agg -q --font-dir "${AGG_FONT_DIR:?}" --font-family "JetBrains Mono,Noto Sans Mono CJK JP" \
  --font-size 16 --idle-time-limit 0.5 --last-frame-duration 1 "$OUT/$NAME.cast" "$OUT/$NAME.gif"
