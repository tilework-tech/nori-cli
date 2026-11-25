#!/bin/bash
# Terminal A controls; B joins as a watcher, presses Alt+T and types at once.
S=~/org/workspace/.claude/skills/tui-puppeteering-with-tmux
H=$HOME/org/workspace/.local/tt/home; BIN=$HOME/org/workspace/cli/nori-rs/target/debug; T=$HOME/org/workspace/.local/tt
ID=$(cd $H/project && NORI_HOME=$H CODEX_HOME=$H $BIN/nori host ls | awk 'NR==2{print $1}')
PORT=$(python3 -c "import json;print(json.load(open('$H/host/state.json'))['endpoint'].split(':')[2].split('/')[0])")
ENV="cd $H/project && PATH=$BIN:\$PATH NORI_HOME=$H CODEX_HOME=$H NORI_NO_ANALYTICS=1"
for run in 1 2 3; do
  $S/tui-start ttA "$ENV $BIN/nori attach $ID" >/dev/null
  $S/tui-assert ttA "controlling" 20 >/dev/null || $S/tmux-isolated send-keys -t ttA M-t
  $S/tui-assert ttA "controlling" 20 >/dev/null
  : > $T/B.methods
  $S/tui-start ttB "$ENV $BIN/nori attach $ID --host ws://127.0.0.1:47001/acp --token-file $H/host/token" >/dev/null
  $S/tui-assert ttB "watching" 20 >/dev/null
  $S/tmux-isolated send-keys -t ttB M-t
  $S/tui-assert ttB "controlling" 20 >/dev/null
  $T/scripts/type_like_a_person.sh ttB "synthetic takeover $run"
  sleep 4
  echo "run $run: prompts=$(grep -c _nori/session/prompt $T/B.methods) A_sees=$($S/tui-capture ttA | grep -c "synthetic takeover $run")"
  $S/tui-stop ttB >/dev/null; $S/tui-stop ttA >/dev/null
done
