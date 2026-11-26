#!/usr/bin/env bash
# Drive the real `nori` binary against the mock ACP agent inside tmux,
# recording the terminal with asciinema so frame timings can be measured.
#
# Usage: bash run.sh <scenario> <cols> <rows> [KEY=VALUE ...] [-- extra config.toml lines]
# Environment: NORI_BIN (path to the debug `nori`; mock_acp_agent must sit next to it),
#              OUT (results directory).
set -euo pipefail

scenario=$1 cols=$2 rows=$3
shift 3
agent_env=()
extra_toml=""
while [ $# -gt 0 ]; do
  if [ "$1" = "--" ]; then shift; extra_toml="$*"; break; fi
  agent_env+=("$1"); shift
done

work=${WORK:-/tmp/v801}/$scenario
rm -rf "$work"
mkdir -p "$work/home" "$work/tidepool"
echo 'print("tide tables")' > "$work/tidepool/tides.py"
project=$(cd "$work/tidepool" && pwd -P)
{
  echo 'agent = "mock-model"'
  echo
  echo "[projects.\"$project\"]"
  echo 'trust_level = "trusted"'
  if [ -n "$extra_toml" ]; then printf '\n%b\n' "$extra_toml"; fi
} > "$work/home/config.toml"

mkdir -p "$OUT/$scenario"
cast=$OUT/$scenario/session.cast
envs="TERM=xterm-256color NORI_HOME=$work/home CODEX_HOME=$work/home NORI_MOCK_INSTRUCTION_FILES=1 NORI_SYNC_SYSTEM_INFO=1 MOCK_AGENT_PROMPT_LOG=$work/prompts.log ${agent_env[*]:-}"
session="v801-$scenario"
tmux kill-session -t "$session" 2>/dev/null || true
tmux new-session -d -s "$session" -x "$cols" -y "$rows" -c "$work/tidepool" \
  "asciinema rec -q --overwrite -c 'env $envs $NORI_BIN --agent mock-model' $cast"
echo "$session $work"
