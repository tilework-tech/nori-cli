#!/bin/bash
# A controls and streams until cancelled; B watches, keeps a draft and presses Ctrl+C, Esc, Ctrl+C.
S=~/org/workspace/.claude/skills/tui-puppeteering-with-tmux
H=$HOME/org/workspace/.local/tt/home; BIN=$HOME/org/workspace/cli/nori-rs/target/debug; T=$HOME/org/workspace/.local/tt
ID=$(cd $H/project && NORI_HOME=$H CODEX_HOME=$H $BIN/nori host ls | awk 'NR==2{print $1}')
PORT=$(python3 -c "import json;print(json.load(open('$H/host/state.json'))['endpoint'].split(':')[2].split('/')[0])")
[ -f $T/relayA.pid ] && kill $(cat $T/relayA.pid) 2>/dev/null
nohup python3 -I $T/scripts/count_relay.py 47002 $PORT $T/A.methods >/dev/null 2>&1 & echo $! > $T/relayA.pid
sleep 0.5
ENV="cd $H/project && PATH=$BIN:\$PATH NORI_HOME=$H CODEX_HOME=$H NORI_NO_ANALYTICS=1"
$S/tui-start ttA "$ENV $BIN/nori attach $ID --host ws://127.0.0.1:47002/acp --token-file $H/host/token" >/dev/null
$S/tui-assert ttA "›" 20 >/dev/null; $S/tmux-isolated send-keys -t ttA M-t; $S/tui-assert ttA "controlling" 20 >/dev/null
$S/tui-start ttB "$ENV $BIN/nori attach $ID --host ws://127.0.0.1:47001/acp --token-file $H/host/token" >/dev/null
$S/tui-assert ttB "watching" 20 >/dev/null
$T/scripts/type_like_a_person.sh ttA "mock:stream-until-cancel"
$S/tui-assert ttB "esc to interrupt" 20 >/dev/null
: > $T/B.methods; : > $T/A.methods
$S/tmux-isolated send-keys -t ttB -l "synthetic watcher draft"; sleep 0.5
$S/tmux-isolated send-keys -t ttB C-c; sleep 0.3; $S/tmux-isolated send-keys -t ttB Escape; sleep 0.3; $S/tmux-isolated send-keys -t ttB C-c; sleep 0.3; $S/tmux-isolated send-keys -t ttB Escape; sleep 2
echo "== B after watcher Ctrl+C/Esc/Ctrl+C: B requests: $(grep -v -c _nori/session/list $T/B.methods) $(grep -v _nori/session/list $T/B.methods | tr '\n' ' ')"
$S/tui-capture ttB | grep -v "^\s*$" | tail -6
echo "== A still streaming:"; $S/tui-capture ttA | grep -v "^\s*$" | tail -3
$S/tui-capture ttB > $T/B-watcher.txt; $S/tui-capture ttB -e > $T/B-watcher.ansi; $S/tui-capture ttA -e > $T/A-streaming.ansi
$S/tmux-isolated send-keys -t ttA -l "synthetic controller draft"; sleep 0.5
$S/tmux-isolated send-keys -t ttA C-c; sleep 2
echo "== A after controller Ctrl+C: A requests: $(grep -v _nori/session/list $T/A.methods | tr '\n' ' ')"
$S/tui-capture ttA | grep -v "^\s*$" | tail -5
$S/tui-capture ttA -e > $T/A-cancelled.ansi
$S/tmux-isolated send-keys -t ttA C-u; sleep 0.3; $S/tmux-isolated send-keys -t ttA C-c; sleep 2
echo "== A after second Ctrl+C:"; $S/tui-capture ttA | grep -v "^\s*$" | tail -12
echo "A requests now: $(grep -v _nori/session/list $T/A.methods | tr '\n' ' ')"
$S/tui-capture ttA -e > $T/A-interrupted.ansi
$S/tmux-isolated send-keys -t ttA C-c; sleep 2
echo "== A after Ctrl+C on an idle empty prompt:"; $S/tui-capture ttA | grep -v "^\s*$" | tail -4
echo "== B still watching:"; $S/tui-capture ttB | grep -v "^\s*$" | tail -2
$S/tui-stop ttB >/dev/null; $S/tui-stop ttA >/dev/null
