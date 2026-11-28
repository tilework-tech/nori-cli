# usage: bash attach.sh ; records raw PTY bytes of a `nori attach` controller on default config
set -eu
out=$PWD/out/attach; rm -rf "$out"; mkdir -p "$out/home/project"
bin=$HOME/target-ps/debug
printf 'agent = "mock-model"\n[projects."%s"]\ntrust_level = "trusted"\n' "$out/home/project" > "$out/home/config.toml"
cp "$out/home/config.toml" "$out/config.toml"
envs="NORI_HOME=$out/home CODEX_HOME=$out/home NORI_NO_ANALYTICS=1 TERM=xterm-256color PATH=$bin:\$PATH MOCK_ACP_AGENT_BIN=$bin/mock_acp_agent MOCK_AGENT_RESPONSE='The tide table lists four high tides.'"
for s in ps-host ps-attach; do tmux kill-session -t $s 2>/dev/null || true; done
wait_for() { for i in $(seq 1 120); do tmux capture-pane -p -t $1 | grep -q "$2" && return 0; sleep 0.25; done; echo "timeout waiting for $2"; tmux capture-pane -p -t $1; return 1; }
tmux new-session -d -s ps-host -x 110 -y 30 -c "$out/home/project" "env $envs $bin/nori --agent mock-model host --port 0; sleep 30"
wait_for ps-host "tarted"
tmux new-session -d -s ps-attach -x 110 -y 30 -c "$out/home/project" "env $envs script -qfec '$bin/nori --agent mock-model attach' $out/raw.bin; sleep 30"
wait_for ps-attach "Sessions"; sleep 1
tmux send-keys -t ps-attach Escape; wait_for ps-attach "No session selected"; sleep 0.5
tmux send-keys -t ps-attach -l "/new"; sleep 0.5; tmux send-keys -t ps-attach Enter
wait_for ps-attach "ready"; sleep 1
tmux send-keys -t ps-attach -l "mock:request-permission:cargo test --offline"; sleep 0.5; tmux send-keys -t ps-attach Enter
wait_for ps-attach "Would you like to allow"; sleep 1.5
tmux capture-pane -p -t ps-attach > "$out/screen-approval.txt"
tmux send-keys -t ps-attach Enter
wait_for ps-attach "Permission granted"; sleep 1
tmux capture-pane -p -t ps-attach > "$out/screen-done.txt"
tmux send-keys -t ps-attach C-d; sleep 2
tmux kill-session -t ps-attach 2>/dev/null || true
env NORI_HOME=$out/home CODEX_HOME=$out/home $bin/nori host stop >/dev/null 2>&1 || true
tmux kill-session -t ps-host 2>/dev/null || true
python3 -I "$(dirname "$0")/reports.py" "$out/raw.bin" | tee "$out/reports.txt"
