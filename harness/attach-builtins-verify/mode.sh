#!/usr/bin/env bash
# Native mode change and zero-option replacement, then later attaches (protocol review cases for #803).
NORI=$1; OUT=$2; mkdir -p "$OUT"; source "$(dirname "$0")/lib.sh"
sid_of() { hostcmd ls 2>/dev/null | awk -v a="$1" '$2==a{print $1}' | tail -1; }
pick() { # pick TERM COMMAND ROW
  cmd "$1" "$2"; sleep 1.5; down_to "$1" "$3"; k "$1" Enter; }
take_control_model() { # late terminal: take control, open /model, snapshot, close
  k "$1" M-t; sleep 2; cmd "$1" "/model"; sleep 1.5; snap "$1" "$2"; k "$1" Escape; sleep 0.5; }

setup_host mode kestrel
# R1 native mode change, then a later attach
term A; wait_for A "Sessions" 20; k A Escape; sleep 0.5
mark; cmd A "/new codex"; wait_for A "Osprey options|codex options" 30; sleep 1; O1=$(new_sid)
cmd A "/config"; sleep 1.5; snap A r1-config-osprey; closed A; k A C-u; sleep 0.3
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
echo "K3=$K3 O1=$O1" > "$SC/r2-ids.txt"; hostcmd ls >> "$SC/r2-ids.txt"
term F "${K3:0:8}"; sleep 5; snap F r2-late-attach
take_control_model F r2-late-model
expect R2d r2-late-attach "now runs Moss" "later attach by ID opens the replaced session itself"
# Reach the replaced session through the list instead, so R2 is judged on the right session.
k F M-s; wait_for F "Sessions" 10; down_to F "moss"; k F Enter; sleep 2; snap F r2-list-open
cmd F "/model"; sleep 1.5; snap F r2-list-model; closed F
expect R2e r2-list-open "controlling .* moss .* ready" "list opens the replaced session"
expect_not R2f r2-list-model "[0-9]\\. harbor" "replaced session (via list) offers no previous-agent models"
k F M-s; sleep 1.5; snap F r2-list-row; k F Escape
expect_not R2c r2-late-model "[0-9]\. harbor" "later attach does not offer the replaced agent's models"
typed F "/con"; sleep 0.8; snap F r2-late-slash; k F C-u
k F C-d; sleep 1
teardown
