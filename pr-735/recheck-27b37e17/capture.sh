#!/bin/bash
# capture.sh BIN SCENARIO COLS OUTNAME [extra args]
BIN=$1; SCN=$2; COLS=$3; OUT=$4; shift 4
T=$HOME/org/workspace/cli/.worktrees/v735-tools
sed -i -E "s#scenario-[a-z]\.json#scenario-$SCN.json#" /tmp/nori-demo/home/config.toml
cd /tmp/nori-demo/project
S=cap$RANDOM
tmux new-session -d -s $S -x $COLS -y ${ROWS:-40} "env -i HOME=/tmp/nori-demo/home TERM=xterm-256color PATH=/usr/bin:/bin NORI_HOME=/tmp/nori-demo/home CODEX_HOME=/tmp/nori-demo/home NORI_NO_ANALYTICS=1 $BIN --agent synthetic $*; sleep 60"
sleep 6; tmux send-keys -t $S -l "explore"; sleep 0.6; tmux send-keys -t $S Enter; sleep 6
tmux capture-pane -p -t $S > $T/cap/$OUT.txt; tmux capture-pane -e -p -t $S > $T/cap/$OUT.ansi
tmux kill-session -t $S
