#!/usr/bin/env bash
# /model and /config in attach (nori-cli #803), incl. the protocol review's two user-facing cases.
NORI=$1; OUT=$2; mkdir -p "$OUT"; source "$(dirname "$0")/lib.sh"
sid_of() { hostcmd ls 2>/dev/null | awk -v a="$1" '$2==a{print $1}' | tail -1; }
pick() { # pick TERM COMMAND ROW
  cmd "$1" "$2"; sleep 1.5; down_to "$1" "$3"; k "$1" Enter; }
take_control_model() { # late terminal: take control, open /model, snapshot, close
  k "$1" M-t; sleep 2; cmd "$1" "/model"; sleep 1.5; snap "$1" "$2"; k "$1" Escape; sleep 0.5; }

setup_host model kestrel
term A; wait_for A "Sessions" 40; sleep 0.5; k A Escape; sleep 0.5
mark; cmd A "/new"; wait_for A "kestrel options" 30; sleep 1; K1=$(new_sid)
# B1 confirmed /model change
pick A "/model" "harbor-max"; wait_for A "option set: Model=harbor-max" 20; sleep 0.5; snap A b1-confirmed; closed A
expect B1a b1-confirmed "option set: Model=harbor-max" "host-confirmed /model change is announced"
cmd A "status"; wait_for A "Kestrel turn 1" 20; snap A b1-reply
expect B1b b1-reply "model harbor-max" "next reply uses the confirmed model"
# B2 agent rejects a model
echo kestrel-classic > "$ROOT/reject-model"
pick A "/model" "kestrel-classic"; sleep 3; snap A b2-rejected; closed A
expect B2a b2-rejected "refuses model kestrel-classic|rejected|failed|error" "agent rejection is shown"
expect_not B2b b2-rejected "option set: Model=kestrel-classic" "rejected value is not announced as set"
cmd A "/model"; sleep 1.5; snap A b2-picker-after; closed A; sleep 0.5
expect B2c b2-picker-after "harbor-max \(current\)" "picker still shows the confirmed model after a rejection"
cmd A "status"; wait_for A "Kestrel turn 2" 20; snap A b2-reply
expect B2d b2-reply "turn 2 .*model harbor-max" "agent still runs the confirmed model"
rm "$ROOT/reject-model"
# B5 /config
cmd A "/config"; sleep 1.5; snap A b5-config
expect B5a b5-config "Model \(harbor-max\)" "/config lists Model with the confirmed value"
expect_not B5b b5-config "Agent|Mode \(" "/config leaves out agent and permission mode"
down_to A "Effort"; k A Enter; sleep 1; down_to A "High"; k A Enter; wait_for A "Effort=High" 20; sleep 0.5; snap A b5-effort; closed A
expect B5c b5-effort "option set: .*Effort=High|Effort=High" "effort change confirmed"
cmd A "status"; wait_for A "Kestrel turn 3" 20; snap A b5-reply
expect B5d b5-reply "turn 3 .*effort high" "next reply uses effort high"
# B4 late attach after a confirmed change
term C "${K1:0:8}"; sleep 5; snap C b4-late-attach
take_control_model C b4-late-model
expect B4a b4-late-model "harbor-max \(current\)" "a later attach (after Alt+T) sees the confirmed model"
k C C-d; sleep 1; k A M-t; sleep 2
# B3 delayed reply across a session switch
mark; cmd A "/new lark"; wait_for A "lark options" 30; sleep 1; L1=$(new_sid)
k A M-s; wait_for A "Sessions" 10; down_to A "kestrel"; k A Enter; sleep 2
echo 5 > "$ROOT/delay-config"
pick A "/model" "harbor-mini"; sleep 0.8; snap A b3-pending
closed A; k A M-s; wait_for A "Sessions" 10; down_to A "lark"; k A Enter; sleep 1
term D "${K1:0:8}"   # late attach while the reply is outstanding
sleep 7; snap A b3-other-session
expect_not B3a b3-other-session "option set: Model=harbor-mini" "delayed answer does not land in the other session"
rm "$ROOT/delay-config"
cmd A "status"; wait_for A "Lark turn 1" 20; snap A b3-other-reply
expect B3b b3-other-reply "Lark turn 1 .*model harbor-mini, effort low" "other session's config untouched"
k A M-s; wait_for A "Sessions" 10; down_to A "kestrel"; k A Enter; sleep 2; snap A b3-back
expect B3c b3-back "option set: Model=harbor-mini" "delayed answer is shown in the session it was made in"
cmd A "status"; wait_for A "Kestrel turn 4" 20; snap A b3-back-reply
expect B3d b3-back-reply "turn 4 .*model harbor-mini" "agent confirms the delayed change"
k A C-d; sleep 1
take_control_model D b3-late-during-pending
expect B3e b3-late-during-pending "harbor-mini \(current\)" "terminal attached while the answer was pending sees it"
k D C-d; sleep 1
# B6 watcher cannot change config
term A "${K1:0:8}"; sleep 4; term W "${K1:0:8}"; sleep 4; typed W "/mod"; sleep 0.8; snap W b6-watcher-slash; k W C-u
expect B6a b6-watcher-slash "/model .*watching" "watcher's /model is greyed"
typed W "/con"; sleep 0.8; snap W b6-watcher-config; k W C-u
expect B6b b6-watcher-config "/config .*watching" "watcher's /config is greyed"
k W C-d; k A C-d; sleep 1
# R1 native mode change, then a later attach
term A; wait_for A "Sessions" 20; k A Escape; sleep 0.5
mark; cmd A "/new codex"; wait_for A "Osprey options|codex options" 30; sleep 1; O1=$(new_sid)
cmd A "/config"; sleep 1.5; snap A r1-config-osprey; k A Escape; sleep 0.3
expect_not R1a r1-config-osprey "Mode \(" "/config hides the native mode option"
cmd A "/approvals"; sleep 1.5; snap A r1-approvals-before; down_to A "Ask"; k A Enter; sleep 3; snap A r1-after-ask; closed A
cmd A "status"; wait_for A "Osprey turn 1" 20; snap A r1-reply
expect R1b r1-reply "mode read-only" "agent confirms the native mode change"
expect_not R1d r1-reply "\[ workspace-write \]" "controller's mode badge does not show the replaced native mode"
[ "$(grep -o 'Approvals: Ask' "$SC/r1-reply.txt" | wc -l)" -le 1 ] && check R1e PASS "footer shows the approval mode once" || check R1e FAIL "footer repeats 'Approvals: Ask'"
k A C-d; sleep 1
term E "${O1:0:8}"; sleep 5; snap E r1-late-attach
k E M-t; sleep 2; snap E r1-late-control
cmd E "/approvals"; sleep 1.5; snap E r1-late-approvals; k E Escape; sleep 0.3
expect R1c r1-late-approvals "Ask \(current\)" "later attach shows Ask as current"
expect_not R1f r1-late-control "\[ workspace-write \]" "later attach's mode badge does not show the replaced native mode"
cmd E "/config"; sleep 1.5; snap E r1-late-config; k E Escape; sleep 0.3
cmd E "/model"; sleep 1.5; snap E r1-late-model; k E Escape; sleep 0.3
k E C-d; sleep 1
# R2 same-ID replacement by an agent with zero options
term A; wait_for A "Sessions" 20; k A Escape; sleep 0.5
mark; cmd A "/new kestrel"; wait_for A "kestrel options" 30; sleep 1; K3=$(new_sid)
pick A "/model" "harbor-max"; wait_for A "option set: Model=harbor-max" 20; sleep 0.5; closed A
pick A "/agent" "Moss"; sleep 4; snap A r2-after-moss; closed A
hostcmd ls > "$SC/r2-host-ls.txt"
grep -q "^$K3 *moss" "$SC/r2-host-ls.txt" && check R2a PASS "same-ID replacement by Moss" || check R2a FAIL "no in-place Moss replacement"
cmd A "/model"; sleep 1.5; snap A r2-controller-model; k A Escape; sleep 0.3
expect_not R2b r2-controller-model "[0-9]\. harbor" "controller no longer offers the previous agent's model"
k A C-d; sleep 1
term F "${K3:0:8}"; sleep 5; snap F r2-late-attach
take_control_model F r2-late-model
expect R2d r2-late-attach "now runs Moss" "later attach by ID opens the replaced session itself"
expect_not R2c r2-late-model "[0-9]\. harbor" "later attach does not offer the replaced agent's models"
typed F "/con"; sleep 0.8; snap F r2-late-slash; k F C-u
k F C-d; sleep 1
# B7 plain nori parity
plain P; sleep 6; cmd P "/model"; sleep 2; snap P b7-plain-model
expect B7a b7-plain-model "harbor-mini.*" "plain nori /model lists the advertised models"
teardown
