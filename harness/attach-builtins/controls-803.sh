#!/usr/bin/env bash
# Run the #803 review controls in one nori-rs checkout and print each verdict.
# usage: controls-803.sh <nori-rs dir> <cargo target dir>
set -uo pipefail
cd "$1"; export CARGO_TARGET_DIR=$2
echo "== $(git rev-parse --short=8 HEAD) $(git log -1 --format=%s | cut -c1-60)"
run() { # package/test-filter label
  out=$(cargo test -j 16 "$@" 2>&1)
  echo "$out" | grep -E "^test .* \.\.\. (ok|FAILED)$" | sed -E 's/^test (.*) \.\.\. (ok|FAILED)$/  \2  \1/; s/^  ok/  pass/; s/^  FAILED/  FAIL/'
}
run -p nori-protocol --lib -- load_resume_and_set_answers_keep_their_request_across_the_wire an_untagged_config_payload_is_never_read_as_a_write
run -p nori-tui --lib -- a_journaled_answer_changes_and_removes a_journaled_load_answer a_restart_under_the_same_id an_untagged_config_answer_from_an_older_journal
cargo build -q -j 16 --bin nori --bin mock_acp_agent 2>/dev/null
run -p tui-pty-e2e --test attach_config -- an_agent_replaced_by_one_without_options a_watcher_adopts a_terminal_that_attaches_later
