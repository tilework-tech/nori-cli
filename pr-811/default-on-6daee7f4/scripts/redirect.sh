# usage: bash redirect.sh ; default config, stdout redirected away from the terminal
set -u
out=$PWD/out/redirect; rm -rf "$out"; mkdir -p "$out/home/project"
bin=$HOME/target-ps/debug
printf 'agent = "mock-model"\n[projects."%s"]\ntrust_level = "trusted"\n' "$out/home/project" > "$out/home/config.toml"
export NORI_HOME=$out/home CODEX_HOME=$out/home NORI_NO_ANALYTICS=1 TERM=xterm-256color PATH=$bin:$PATH MOCK_ACP_AGENT_BIN=$bin/mock_acp_agent MOCK_AGENT_RESPONSE='The tide table lists four high tides.'
cd "$out/home/project"
# 1. interactive TUI with a terminal on stdin/stderr but stdout sent to a file
tmux kill-session -t ps-redir 2>/dev/null
tmux new-session -d -s ps-redir -x 110 -y 30 "$bin/nori --agent mock-model > $out/tui-stdout.bin 2> $out/tui-stderr.txt; echo exit=\$? > $out/tui-exit.txt; sleep 30"
sleep 6; tmux send-keys -t ps-redir -l "how many high tides"; sleep 0.5; tmux send-keys -t ps-redir Enter; sleep 3
tmux send-keys -t ps-redir C-d; sleep 0.5; tmux send-keys -t ps-redir C-d; sleep 3
tmux capture-pane -p -t ps-redir > "$out/tui-screen.txt"; tmux kill-session -t ps-redir
# 2. nori exec piped
$bin/nori --agent mock-model exec "how many high tides" > "$out/exec-stdout.txt" 2> "$out/exec-stderr.txt" < /dev/null; echo "exec exit=$?" > "$out/exec-exit.txt"
for f in tui-stdout.bin tui-stderr.txt exec-stdout.txt exec-stderr.txt; do printf '%-16s %6s bytes, OSC 7501 records: %s\n' $f $(wc -c < $out/$f) $(grep -c $'\e]7501' $out/$f); done
cat $out/tui-exit.txt $out/exec-exit.txt
