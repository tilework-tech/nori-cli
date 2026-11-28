# usage: bash plain.sh <name> <extra-toml> ; records raw PTY bytes of plain `nori`
set -eu
name=$1; extra=${2:-}
out=$PWD/out/$name; rm -rf "$out"; mkdir -p "$out/home/project"
bin=$HOME/target-ps/debug
printf 'agent = "mock-model"\n[projects."%s"]\ntrust_level = "trusted"\n%b' "$out/home/project" "$extra" > "$out/home/config.toml"
cp "$out/home/config.toml" "$out/config.toml"
s=ps-$name; tmux kill-session -t $s 2>/dev/null || true
tmux new-session -d -s $s -x 110 -y 30 -c "$out/home/project" \
  "env NORI_HOME=$out/home CODEX_HOME=$out/home NORI_NO_ANALYTICS=1 TERM=xterm-256color PATH=$bin:\$PATH MOCK_ACP_AGENT_BIN=$bin/mock_acp_agent MOCK_AGENT_RESPONSE='The tide table lists four high tides.' script -qfec '$bin/nori --agent mock-model' $out/raw.bin; sleep 30"
wait_for() { for i in $(seq 1 120); do tmux capture-pane -p -t $s | grep -q "$1" && return 0; sleep 0.25; done; echo "timeout waiting for $1"; tmux capture-pane -p -t $s; return 1; }
wait_for "〉"; sleep 1
tmux send-keys -t $s -l "mock:request-permission:cargo test --offline"; sleep 0.5; tmux send-keys -t $s Enter
wait_for "Would you like to allow"; sleep 1.5
tmux capture-pane -p -t $s > "$out/screen-approval.txt"
tmux send-keys -t $s 1
wait_for "Permission granted"; sleep 1
tmux capture-pane -p -t $s > "$out/screen-done.txt"
tmux send-keys -t $s C-d; sleep 0.5; tmux send-keys -t $s C-d; sleep 2
tmux kill-session -t $s 2>/dev/null || true
python3 -I "$(dirname "$0")/reports.py" "$out/raw.bin" | tee "$out/reports.txt"
