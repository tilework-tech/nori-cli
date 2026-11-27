#!/usr/bin/env bash
# In-place /agent change and the new agent's native mode: initiating controller,
# host receipt (/approvals), fresh attaches, and the hint row over time.
NORI=$1; OUT=$2; mkdir -p "$OUT"; source "$(dirname "$0")/lib.sh"
pick() { cmd "$1" "$2"; sleep 1.5; down_to "$1" "$3"; k "$1" Enter; }
hint() { screen "$1" | grep -E 'controlling|watching' | tail -1; }
badge() { screen "$1" | grep -oE '\[ [a-z-]+ \]' | tail -1; }
setup_host inplace kestrel
term A; wait_for A "Sessions" 40; k A Escape; sleep 0.5
mark; cmd A "/new kestrel"; wait_for A "kestrel options" 30; sleep 1; K=$(new_sid)
snap A m0-before; t0=$(date +%s.%N)
pick A "/agent" "Osprey"
: > "$SC/m1-hint-timeline.txt"
for i in $(seq 1 24); do printf '%5.2fs  badge=%-20s hint=%s\n' "$(awk -v a="$(date +%s.%N)" -v b="$t0" 'BEGIN{print a-b}')" "$(badge A)" "$(hint A)" >> "$SC/m1-hint-timeline.txt"; sleep 0.5; done
closed A; snap A m1-after-switch
expect M1a m1-after-switch "now runs Osprey" "in-place change to Osprey announced"
expect M1b m1-after-switch "\[ workspace-write \]" "controller shows Osprey's own native mode"
tail -1 "$SC/m1-hint-timeline.txt" | grep -q 'codex' && check M1c PASS "controller hint row names the new agent after 12 s" || check M1c FAIL "hint row after 12 s: $(tail -1 "$SC/m1-hint-timeline.txt")"
cmd A "status"; wait_for A "turn 1" 20; sleep 0.5; snap A m2-first-prompt
expect M2a m2-first-prompt "Osprey turn 1 .*mode workspace-write" "next prompt goes to Osprey in its own mode"
expect_not M2b m2-first-prompt "Kestrel turn" "next prompt does not reach Kestrel"
printf 'after prompt: badge=%s hint=%s\n' "$(badge A)" "$(hint A)" >> "$SC/m1-hint-timeline.txt"
expect M2c m2-first-prompt "controlling .* codex .* ready" "hint row names the new agent after the first prompt"
# Host receipt: /approvals ask maps onto Osprey's read-only mode.
cmd A "/approvals"; sleep 1.5; down_to A "Ask"; k A Enter; sleep 3; closed A; snap A m3-after-receipt
expect M3a m3-after-receipt "\[ read-only \]" "controller badge follows the host receipt (read-only)"
cmd A "status"; wait_for A "turn 2" 20; snap A m3-reply
expect M3b m3-reply "turn 2 .*mode read-only" "agent confirms read-only"
k A C-d; sleep 1
# Fresh attach through the list
term F; wait_for F "Sessions" 20; down_to F "codex"; k F Enter; sleep 3; snap F m4-fresh-list
expect M4a m4-fresh-list "\[ read-only \]" "fresh attach (list) shows the new agent's current native mode"
expect M4b m4-fresh-list "controlling .* codex" "fresh attach hint row names the new agent"
k F C-d; sleep 1
# Fresh attach by ID (only one session on the host)
term G "${K:0:8}"; sleep 5; snap G m5-fresh-id
expect M5a m5-fresh-id "\[ read-only \]" "fresh attach by ID shows the new agent's current native mode"
expect M5b m5-fresh-id "Osprey turn 2" "fresh attach by ID opens that session"
k G C-d; sleep 1
# Back to an agent without a native mode: the badge must go.
term A; wait_for A "Sessions" 20; down_to A "codex"; k A Enter; sleep 2
cmd A "/new kestrel"; wait_for A "kestrel options" 30; sleep 1
pick A "/agent" "Osprey"; sleep 4; closed A; pick A "/agent" "Kestrel"; sleep 4; closed A; snap A m6-back-to-kestrel
expect_not M6a m6-back-to-kestrel "\[ (workspace-write|read-only) \]" "badge clears when the replacement has no native mode"
teardown
