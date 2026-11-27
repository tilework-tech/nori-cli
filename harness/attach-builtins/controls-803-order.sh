#!/usr/bin/env bash
# The #803 agent-switch ordering controls in one nori-rs checkout.
# usage: controls-803-order.sh <nori-rs dir> <cargo target dir>
set -uo pipefail
cd "$1"; export CARGO_TARGET_DIR=$2
echo "== $(git rev-parse --short=8 HEAD) $(git log -1 --format=%s | cut -c1-62)"
run() { cargo test -j 16 "$@" 2>&1 | grep -E "^test .* \.\.\. (ok|FAILED)$" | sed -E 's/^test (.*) \.\.\. (ok|FAILED)$/  \2  \1/; s/^  ok/  pass/; s/^  FAILED/  FAIL/'; }
run -p nori-tui --lib -- host_config::the_new_agents_own_mode host_config::a_new_agent_without_options host_config::a_restart_under_the_same_id
cargo build -q -j 16 --bin nori --bin mock_acp_agent 2>/dev/null
run -p tui-pty-e2e --test attach_config -- the_controller_keeps an_agent_replaced_by_one_without_options a_terminal_that_attaches_later
