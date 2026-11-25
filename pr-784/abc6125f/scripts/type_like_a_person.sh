#!/bin/bash
# Type $2 into tmux session $1 one key at a time, then Enter at once.
S=~/org/workspace/.claude/skills/tui-puppeteering-with-tmux
text=$2
for ((i=0; i<${#text}; i++)); do
  $S/tmux-isolated send-keys -t "$1" -l -- "${text:i:1}"
  sleep 0.03
done
$S/tmux-isolated send-keys -t "$1" Enter
