#!/bin/bash
# Fresh host per run; the first attached terminal types a prompt at typing speed the moment it reads controlling.
S=~/org/workspace/.claude/skills/tui-puppeteering-with-tmux
H=$HOME/org/workspace/.local/tt/home; BIN=$HOME/org/workspace/cli/nori-rs/target/debug; T=$HOME/org/workspace/.local/tt
for run in 1 2 3; do
  (cd $H/project && NORI_HOME=$H CODEX_HOME=$H $BIN/nori host stop >/dev/null 2>&1; NORI_HOME=$H CODEX_HOME=$H NORI_NO_ANALYTICS=1 PATH=$BIN:$PATH $BIN/nori host --port 0 >/dev/null 2>&1; NORI_HOME=$H CODEX_HOME=$H PATH=$BIN:$PATH $BIN/nori host new >/dev/null 2>&1)
  PORT=$(python3 -c "import json;print(json.load(open('$H/host/state.json'))['endpoint'].split(':')[2].split('/')[0])")
  ID=$(cd $H/project && NORI_HOME=$H CODEX_HOME=$H $BIN/nori host ls | awk 'NR==2{print $1}')
  [ -f $T/relay.pid ] && kill $(cat $T/relay.pid) 2>/dev/null; : > $T/B.methods
  nohup python3 -I $T/scripts/count_relay.py 47001 $PORT $T/B.methods >/dev/null 2>&1 & echo $! > $T/relay.pid
  sleep 0.5
  $S/tui-start ttB "cd $H/project && PATH=$BIN:\$PATH NORI_HOME=$H CODEX_HOME=$H NORI_NO_ANALYTICS=1 $BIN/nori attach $ID --host ws://127.0.0.1:47001/acp --token-file $H/host/token" >/dev/null
  $S/tui-assert ttB "controlling" 20 >/dev/null
  $T/scripts/type_like_a_person.sh ttB "synthetic early hello $run"
  sleep 4
  echo "run $run id=${ID:0:8}: prompts=$(grep -c _nori/session/prompt $T/B.methods) reply=$($S/tui-capture ttB | grep -c 'Test message')"
  $S/tui-capture ttB | grep -v "^\s*$" | tail -3
  $S/tui-stop ttB >/dev/null
done
