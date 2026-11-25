#!/bin/bash
# Two-terminal proof captures for #784 at 2b9f19f1, synthetic data only.
set -e
S=~/org/workspace/.claude/skills/tui-puppeteering-with-tmux
C=~/org/workspace/.claude/skills/tui-capture-with-ghostty-web/scripts/capture-session
H=$HOME/org/workspace/.local/tt/home; BIN=$HOME/org/workspace/cli/nori-rs/target/debug; T=$HOME/org/workspace/.local/tt
OUT=$T/proof/pr-784/2b9f19f1; rm -rf $OUT; mkdir -p $OUT
ENV="cd $H/project && env -u NO_COLOR COLORTERM=truecolor PATH=$BIN:\$PATH NORI_HOME=$H CODEX_HOME=$H NORI_NO_ANALYTICS=1"
(cd $H/project && NORI_HOME=$H CODEX_HOME=$H $BIN/nori host stop >/dev/null 2>&1 || true; NORI_HOME=$H CODEX_HOME=$H NORI_NO_ANALYTICS=1 PATH=$BIN:$PATH $BIN/nori host --port 0 >/dev/null 2>&1; NORI_HOME=$H CODEX_HOME=$H PATH=$BIN:$PATH $BIN/nori host new >/dev/null 2>&1)
ID=$(cd $H/project && NORI_HOME=$H CODEX_HOME=$H $BIN/nori host ls | awk 'NR==2{print $1}')
$S/tui-start ttA "$ENV $BIN/nori attach $ID" >/dev/null
$S/tui-assert ttA "controlling" 20 >/dev/null
$T/scripts/type_like_a_person.sh ttA "Survey the synthetic orchard"
$S/tui-assert ttA "Test message" 20 >/dev/null
$S/tui-start ttB "$ENV $BIN/nori attach $ID" >/dev/null
$S/tui-assert ttB "watching" 20 >/dev/null
$T/scripts/type_like_a_person.sh ttA "mock:stream-until-cancel"
$S/tui-assert ttB "esc to interrupt" 20 >/dev/null
$S/tmux-isolated send-keys -t ttB -l "Synthetic watcher draft"; sleep 0.5
$S/tmux-isolated send-keys -t ttB C-c; sleep 0.3; $S/tmux-isolated send-keys -t ttB Escape; sleep 1
$C ttB $OUT/01-watcher-ctrl-c-esc --cols 120 --rows 40 --expect "Alt+T to take control" --require-color
$C ttA $OUT/02-controller-still-streaming --cols 120 --rows 40 --expect "esc to interrupt" --require-color
$S/tmux-isolated send-keys -t ttB C-u; sleep 0.3; $S/tmux-isolated send-keys -t ttB PageUp; sleep 1
$C ttB $OUT/03-joined-pager --cols 120 --rows 40 --expect "q/esc to quit" --require-color
$S/tmux-isolated send-keys -t ttB Escape; sleep 0.5
$S/tmux-isolated send-keys -t ttA C-c; sleep 2
$C ttA $OUT/04-controller-interrupted --cols 120 --rows 40 --expect "Conversation interrupted" --require-color
$S/tui-stop ttB >/dev/null; $S/tui-stop ttA >/dev/null
(cd $H/project && NORI_HOME=$H CODEX_HOME=$H $BIN/nori host stop >/dev/null 2>&1; NORI_HOME=$H CODEX_HOME=$H NORI_NO_ANALYTICS=1 PATH=$BIN:$PATH $BIN/nori host --port 0 >/dev/null 2>&1; NORI_HOME=$H CODEX_HOME=$H PATH=$BIN:$PATH $BIN/nori host new >/dev/null 2>&1)
ID=$(cd $H/project && NORI_HOME=$H CODEX_HOME=$H $BIN/nori host ls | awk 'NR==2{print $1}')
$S/tui-start ttB "$ENV $BIN/nori attach $ID" >/dev/null
$S/tui-assert ttB "controlling" 20 >/dev/null
$T/scripts/type_like_a_person.sh ttB "Synthetic first prompt"
$S/tui-assert ttB "Test message" 20 >/dev/null
$C ttB $OUT/05-first-prompt-once-controlling --cols 120 --rows 40 --expect "Synthetic first prompt" --require-color
$S/tui-stop ttB >/dev/null
echo PROOF DONE
