# Record the notice storybook's startup scene in a real terminal.
#
# Usage: bash record.sh <storybook-binary> <out-dir> [cols] [rows]
# Needs the tui-puppeteering-with-tmux scripts in $TUI, plus asciinema and agg
# on PATH. Fonts: set AGG_FONT_DIR to a directory with JetBrains Mono and a CJK
# monospace font so the 〉 prompt renders.
set -euo pipefail
BIN=$1 OUT=$2 COLS=${3:-80} ROWS=${4:-26}
TUI=${TUI:?set TUI to the tui-puppeteering-with-tmux directory}
SESSION=notice-record
mkdir -p "$OUT"
CAST="$OUT/startup-${COLS}.cast"

type_slowly() {
  local text=$1 i
  for ((i = 0; i < ${#text}; i++)); do
    "$TUI/tui-send" "$SESSION" "${text:i:1}"
    sleep 0.14
  done
}

"$TUI/tui-start" "$SESSION" "sleep 0.5; asciinema rec --overwrite -q --window-size ${COLS}x${ROWS} -c '$BIN --scene startup --notice cli --also invitation --arrive-ms 1800' $CAST" >/dev/null
"$TUI/tmux-isolated" resize-window -t "$SESSION" -x "$COLS" -y "$ROWS"
"$TUI/tui-assert" "$SESSION" "active notice: none" 10

# Frame one: the composer already has focus. Type across the notice's arrival.
sleep 0.6
type_slowly "fix the flaky retry test"
"$TUI/tui-assert" "$SESSION" "Version 0.31.0" 5
"$TUI/tui-assert" "$SESSION" "fix the flaky retry test" 2
sleep 1.2

# Enter sends the draft once and settles both notices ahead of it.
"$TUI/tui-send" "$SESSION" --keys Enter
"$TUI/tui-assert" "$SESSION" "sends 1" 5
sleep 1.0
"$TUI/tui-send" "$SESSION" --keys Enter
sleep 1.6
"$TUI/tui-assert" "$SESSION" "sends 1" 2

# Replay; this time take the actions on purpose from an empty prompt.
"$TUI/tui-send" "$SESSION" --keys C-r
"$TUI/tui-assert" "$SESSION" "Version 0.31.0" 5
sleep 1.4
"$TUI/tui-send" "$SESSION" --keys Tab
"$TUI/tui-assert" "$SESSION" "mock action cli.update" 5
sleep 1.6
"$TUI/tui-send" "$SESSION" --keys Tab
"$TUI/tui-assert" "$SESSION" "mock action repo.open" 5
sleep 2.0
"$TUI/tui-capture" "$SESSION" > "$OUT/startup-${COLS}-final.txt"
"$TUI/tui-send" "$SESSION" --keys C-c
sleep 1
"$TUI/tui-stop" "$SESSION" >/dev/null

agg --font-dir "${AGG_FONT_DIR:?}" --font-family "JetBrains Mono,Noto Sans Mono CJK JP" \
  --font-size 16 --idle-time-limit 2 --last-frame-duration 3 "$CAST" "$OUT/startup-${COLS}.gif"
echo "$OUT/startup-${COLS}.gif"
