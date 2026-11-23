#!/usr/bin/env bash
# Usage: drive.sh <label> <binary> <cols> <rows> <scenario>...
# Captures every gated frame of each scenario to $OUT/<label>/<scenario>-<n>.txt
set -u
LABEL=$1 BIN=$2 COLS=$3 ROWS=$4; shift 4
S=/home/sprite/org/workspace/.claude/skills/tui-puppeteering-with-tmux
F=/home/sprite/org/workspace/.worktrees/pr730-fixtures
OUT=$F/out/$LABEL
SES=pr730-$LABEL
rm -rf "$F/gates" "$OUT"; mkdir -p "$F/gates" "$OUT"
cd "$F/proj"
$S/tui-start "$SES" "env -u NO_COLOR COLORTERM=truecolor NORI_HOME=$F/home CODEX_HOME=$F/home $BIN --agent synth --skip-trust-directory" >/dev/null
$S/tmux-isolated resize-window -t "$SES" -x "$COLS" -y "$ROWS"
$S/tui-assert "$SES" "›" 20 >/dev/null || { echo "no prompt"; $S/tui-capture "$SES"; }
gate=0
for sc in "$@"; do
  $S/tui-send "$SES" "$sc"; sleep 0.6; $S/tui-send "$SES" --keys Enter
  n=0
  case $sc in reply) done_re="Ordinary synthetic reply";; *) done_re="${sc^} scenario answered";; esac
  while :; do
    # wait for agent to block on the next gate, or for the turn to finish
    for _ in $(seq 1 100); do
      w=$(cut -d' ' -f1 "$F/gates/waiting" 2>/dev/null || echo 0)
      [ "$w" -gt "$gate" ] && break
      $S/tui-capture "$SES" | grep -q "$done_re" && [ "$n" -gt 0 ] && break 2
      sleep 0.1
    done
    w=$(cut -d' ' -f1 "$F/gates/waiting" 2>/dev/null || echo 0)
    [ "$w" -gt "$gate" ] || break
    gate=$w; n=$((n+1))
    sleep 1.2  # let the TUI render the streamed update
    $S/tui-capture "$SES" > "$OUT/$sc-$n.txt"
    $S/tui-capture "$SES" -e > "$OUT/$sc-$n.ansi"
    echo "== $LABEL $sc frame $n ($(cut -d' ' -f2- "$F/gates/waiting")): $(grep -E '•|◦|·' "$OUT/$sc-$n.txt" | grep -i 'interrupt\|[0-9]s)' | tail -1)"
    touch "$F/gates/$gate"
  done
  sleep 1.5
  $S/tui-capture "$SES" > "$OUT/$sc-done.txt"
done
[ -n "${KEEP:-}" ] || $S/tui-stop "$SES" >/dev/null
