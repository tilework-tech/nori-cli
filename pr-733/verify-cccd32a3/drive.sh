#!/usr/bin/env bash
# Usage: drive.sh <label> <cmd> <scenario>...  (run from fixtures dir)
set -u
LABEL=$1 CMD=$2; shift 2
S=/home/sprite/org/workspace/.claude/skills/tui-puppeteering-with-tmux
F=/fixtures
OUT=$F/out/$LABEL; SES=p733-$LABEL
rm -rf "$F/gates" "$OUT"; mkdir -p "$F/gates" "$OUT"
cd "$F/proj"
$S/tui-start "$SES" "$CMD" >/dev/null
$S/tmux-isolated resize-window -t "$SES" -x ${COLS:-100} -y ${ROWS:-50}
$S/tui-assert "$SES" "›" 30 >/dev/null || { echo "no prompt"; $S/tui-capture "$SES"; exit 1; }
for sc in "$@"; do
  $S/tui-send "$SES" "$sc"; sleep 0.6; $S/tui-send "$SES" --keys Enter
  case $sc in reply) re="Ordinary synthetic reply";; *) re="${sc^} scenario answered";; esac
  for _ in $(seq 1 100); do
    if [ -f "$F/gates/waiting" ]; then
      g=$(cat "$F/gates/waiting"); rm "$F/gates/waiting"; sleep 1.0
      $S/tui-capture "$SES" > "$OUT/$sc-indicator.txt"; touch "$F/gates/$g"
    fi
    $S/tui-capture "$SES" | grep -q "$re" && break; sleep 0.1
  done
  sleep 1.0
done
$S/tui-capture "$SES" -S 400 > "$OUT/final.txt"; $S/tui-capture "$SES" -e -S 400 > "$OUT/final.ansi"
if [ -n "${CTRLT:-}" ]; then
  $S/tui-send "$SES" --keys C-t; sleep 1.5
  $S/tui-capture "$SES" > "$OUT/ctrlt.txt"; $S/tui-capture "$SES" -e > "$OUT/ctrlt.ansi"
  $S/tui-send "$SES" --keys Home; sleep 0.8; $S/tui-capture "$SES" > "$OUT/ctrlt-top.txt"
  $S/tui-send "$SES" --keys Escape; sleep 0.5
fi
[ -n "${KEEP:-}" ] || $S/tui-stop "$SES" >/dev/null
