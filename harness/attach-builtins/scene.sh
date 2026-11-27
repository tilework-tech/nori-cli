#!/usr/bin/env bash
# Record one attach scene against a background host whose agents are the
# synthetic fixture servers Amber, Birch (python fixture) and Cedar (absent).
# usage: scene.sh <nori binary> <out dir> <scene>
set -euo pipefail
NORI=$1; OUT=$2; SCENE=$3
FIXTURE=$HOME/org/workspace/cli/nori-rs/cli/tests/standard_acp/fixture.py
mkdir -p "$OUT"
ROOT=$(mktemp -d /tmp/proof-XXXX)
HOSTHOME=$ROOT/host; PROJECT=$HOSTHOME/project
CLIENT=$ROOT/client; FOLDER=$ROOT/terminal
mkdir -p "$PROJECT" "$CLIENT" "$FOLDER"
for gate in fixture-config-options-only fixture-settings-report; do
  case $SCENE in model|replace) touch "$PROJECT/$gate" ;; esac
done
agent() { printf '[[agents]]\nname = "%s"\nslug = "%s"\n[agents.distribution.local]\ncommand = "python3"\nargs = ["%s", "%s", "%s"]\n' "$1" "$2" "$FIXTURE" "$2" "$PROJECT"; }
{
  printf 'agent = "amber"\n[projects."%s"]\ntrust_level = "trusted"\n' "$PROJECT"
  agent Amber amber; agent Birch birch
  printf '[[agents]]\nname = "Cedar"\nslug = "cedar"\n[agents.distribution.local]\ncommand = "cedar-adapter-absent"\n'
} > "$HOSTHOME/config.toml"
echo 'agent = "amber"' > "$CLIENT/config.toml"
[ "$SCENE" = replace ] && touch "$PROJECT/fixture-no-options-birch"
echo "synthetic chart" > "$PROJECT/harbor-chart-on-host.md"
echo "synthetic chart" > "$FOLDER/harbor-chart-on-client.md"
export NORI_NO_ANALYTICS=1
(cd "$PROJECT" && HOME=$HOSTHOME NORI_HOME=$HOSTHOME "$NORI" --agent amber host --port 0 >/dev/null 2>&1)
for _ in $(seq 50); do [ -s "$HOSTHOME/host/state.json" ] && break; sleep 0.2; done
ENDPOINT=$(python3 -c 'import json,sys;print(json.load(open(sys.argv[1]))["endpoint"])' "$HOSTHOME/host/state.json")
S=proof-$SCENE
CMD="cd $FOLDER && HOME=$CLIENT NORI_HOME=$CLIENT $NORI attach --host $ENDPOINT --token-file $HOSTHOME/host/token"
if [ "$SCENE" = replace ]; then
  printf '#!/usr/bin/env bash\n%s\necho; echo "-- a second terminal attaches --"; sleep 1\n%s\n' "$CMD" "$CMD" > "$ROOT/twice.sh"
  chmod +x "$ROOT/twice.sh"; CMD=$ROOT/twice.sh
fi
tmux kill-session -t "$S" 2>/dev/null || true
tmux new-session -d -s "$S" -x 110 -y 44 "asciinema rec -q --overwrite --cols 110 --rows 44 -c '$CMD' $OUT/$SCENE.cast"
tmux set -t "$S" window-style bg=#141414
k() { tmux send-keys -t "$S" "$@"; sleep 0.35; }
typed() { for ((i=0; i<${#1}; i++)); do tmux send-keys -t "$S" -l "${1:i:1}"; sleep 0.09; done; sleep 0.6; }
wait_for() { for _ in $(seq 80); do tmux capture-pane -t "$S" -p | grep -qF "$1" && return 0; sleep 0.25; done; echo "missing: $1" >&2; tmux capture-pane -t "$S" -p >&2; return 1; }
down_to() { for key in Down Up; do for _ in $(seq 20); do tmux capture-pane -t "$S" -p | grep '›' | grep -qF "$1" && return 0; k $key; done; done; return 1; }
snap() { tmux capture-pane -t "$S" -p > "$OUT/$SCENE-$1.txt"; }
wait_for "Sessions"; sleep 1; k Escape; wait_for "No session selected"
case $SCENE in
  agent)
    typed "/agents"; k Enter; wait_for "Choose an agent"; sleep 1
    down_to "Cedar"; sleep 1.2; snap picker-cedar; k Enter; sleep 1.5; snap cedar-refused
    down_to "Birch"; sleep 1; k Enter; wait_for "birch · ready"; sleep 1
    typed "whoami"; k Enter; wait_for "birch is answering (1)."; sleep 1
    typed "/agent"; k Enter; wait_for "Choose an agent"; sleep 1; down_to "Amber"; sleep 1; snap picker-prompted; k Enter
    wait_for "Started a new session with Amber"; sleep 1
    typed "whoami"; k Enter; wait_for "amber is answering (1)."; sleep 1.5; snap beside
    k M-s; wait_for "Sessions"; sleep 2; snap list; k Escape ;;
  model)
    typed "/new"; k Enter; wait_for "amber · ready"; sleep 1
    typed "whoami"; k Enter; wait_for "with model initial"; sleep 1
    typed "/config"; k Enter; wait_for "Session config"; sleep 1.2; snap config
    k Enter; sleep 1; down_to "Amber"; sleep 1; k Enter; wait_for "option set: Model=Amber"; sleep 1.5; snap confirmed
    k Escape; sleep 0.8; typed "whoami"; k Enter; wait_for "with model amber"; sleep 1.5; snap reply ;;
  replace)
    typed "/new"; k Enter; wait_for "amber · ready"; sleep 1
    typed "/model"; k Enter; wait_for "Select a value"; down_to "Amber"; sleep 0.8; k Enter; wait_for "option set: Model=Amber"; sleep 1; k Escape; sleep 0.8
    typed "/agent"; k Enter; wait_for "Choose an agent"; down_to "Birch"; sleep 0.8; k Enter; wait_for "This session now runs Birch."; sleep 1
    typed "/model"; k Enter; sleep 2; snap live; k Escape; sleep 0.8
    k C-d; wait_for "Sessions"; sleep 1.5; down_to "new session"; k Enter; sleep 2
    typed "/model"; k Enter; sleep 2; snap late; k Escape ;;
  mention)
    typed "/mention"; k Enter; sleep 0.8; typed "harbor"; wait_for "harbor-chart-on-host.md"; sleep 1.5; snap no-session
    k Escape; sleep 0.5; for _ in $(seq 10); do k BSpace; done
    typed "/new"; k Enter; wait_for "amber · ready"; sleep 1
    typed "/mention"; k Enter; sleep 0.8; typed "harbor"; wait_for "harbor-chart-on-host.md"; sleep 1.5; snap session
    k Tab; sleep 1.5; snap inserted ;;
esac
sleep 1; k C-d; sleep 2
tmux kill-session -t "$S" 2>/dev/null || true
(cd "$PROJECT" && HOME=$HOSTHOME NORI_HOME=$HOSTHOME "$NORI" host stop >/dev/null 2>&1) || true
agg --font-family "JetBrains Mono,Noto Sans Mono CJK JP" --font-size 15 --idle-time-limit 1.5 --last-frame-duration 4 "$OUT/$SCENE.cast" "$OUT/$SCENE.gif" >/dev/null 2>&1
rm -rf "$ROOT"
echo "done $OUT/$SCENE.gif"
