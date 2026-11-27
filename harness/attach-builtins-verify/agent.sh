#!/usr/bin/env bash
# /agent and /agents in attach (nori-cli #802).
NORI=$1; OUT=$2; mkdir -p "$OUT"; source "$(dirname "$0")/lib.sh"
sid_of() { hostcmd ls 2>/dev/null | awk -v a="$1" '$2==a{print $1}' | tail -1; }
count() { hostcmd ls 2>/dev/null | tail -n +2 | grep -c . ; }

setup_host agent kestrel
term A; wait_for A "Sessions" 40; sleep 0.5; k A Escape; sleep 0.5
typed A "/age"; sleep 0.8; snap A a2-slash-filter; k A C-u
[ "$(grep -cE '^ +/agents? ' "$SC/a2-slash-filter.txt")" = 1 ] && check A2a PASS "slash menu lists /agent once" || check A2a FAIL "slash menu /agent row count != 1"
cmd A "/agents"; wait_for A "Choose an agent" 20; sleep 0.5; snap A a1-picker
down_to A "Cedar"; sleep 0.5; snap A a6-picker-cedar
expect A6a a6-picker-cedar "not installed|cannot|not found" "Cedar row shows the host's reason"
k A Enter; sleep 1.5; snap A a6-cedar-enter
expect A6b a6-cedar-enter "Cedar cannot start on this host" "Enter on Cedar explains the refusal"
expect A6c a6-cedar-enter "Choose an agent" "picker stays open after the refusal"
[ "$(count)" = 0 ] && check A6d PASS "refusal starts no session" || check A6d FAIL "refusal started a session"
down_to A "Lark"; k A Enter; wait_for A "lark" 30; sleep 2; snap A a1-lark-started
[ -n "$(sid_of lark)" ] && check A1a PASS "empty host: /agents started a Lark session on the host" || check A1a FAIL "no lark session on host"
cmd A "status"; wait_for A "Lark turn 1" 20; snap A a1-lark-reply
expect A1b a1-lark-reply "Lark turn 1 on lark-1" "Lark answers in the new session"

# In-place change before the first prompt, with a watcher already on the session.
mark; cmd A "/new"; wait_for A "kestrel options" 30; sleep 1
K1=$(new_sid); echo "$K1" > "$SC/inplace-id-before.txt"
term B "${K1:0:8}"; sleep 4; snap B a5-watcher-before
cmd A "/agent"; wait_for A "Choose an agent" 20; down_to A "Lark"; k A Enter; sleep 3; snap A a3-after-switch
hostcmd ls > "$SC/a3-host-ls.txt"
grep -q "^$K1 *lark" "$SC/a3-host-ls.txt" && check A3a PASS "unprompted session changed agent in place under the same ID" || check A3a FAIL "ID $K1 not running lark"
[ "$(count)" = 2 ] && check A3b PASS "in-place change created no extra session" || check A3b FAIL "session count $(count), expected 2"
cmd A "status"; wait_for A "Lark turn 1" 20; snap A a3-reply
expect A3c a3-reply "Lark turn 1" "the next prompt reaches Lark, not Kestrel"
expect A3d a3-reply "controlling .* lark .* ready" "controller footer names the new agent after the in-place change"
typed A "/mod"; sleep 0.8; snap A a3-controller-slash; k A C-u
expect A3e a3-controller-slash "/model .*current: lark" "controller slash menu names the new agent"
sleep 1; snap B a5-watcher-after
expect_not A5a a5-watcher-after "Session ended" "watcher keeps the session after the in-place change"
expect A5b a5-watcher-after "Lark turn 1" "watcher sees the new agent's reply"
typed B "/age"; sleep 0.8; snap B a5-watcher-slash; k B C-u
expect A5c a5-watcher-slash "/agent .*watching" "watcher's /agent is greyed with the take-control hint"
term C "${K1:0:8}"; sleep 5; snap C a7-late-attach
expect_not A7a a7-late-attach "Session ended" "a later attach opens the in-place-changed session"
expect A7b a7-late-attach "now runs Lark" "a later attach by ID opens the in-place-changed session itself"

# Prompted session: the new agent starts beside it.
mark; cmd A "/new kestrel"; wait_for A "kestrel options" 30; sleep 1; K2=$(new_sid)
cmd A "status"; wait_for A "Kestrel turn 1" 20
cmd A "/agent"; wait_for A "Choose an agent" 20; down_to A "Lark"; sleep 0.5; snap A a4-picker-prompted; k A Enter
wait_for A "Started a new session with Lark" 30; sleep 1; snap A a4-beside
expect A4a a4-beside "Started a new session with Lark" "prompted session: notice names the new agent and Alt+S"
hostcmd ls > "$SC/a4-host-ls.txt"
grep -q "^$K2 *kestrel *ready" "$SC/a4-host-ls.txt" && check A4b PASS "previous conversation keeps running" || check A4b FAIL "previous kestrel session not ready"
[ "$(count)" = 4 ] && check A4c PASS "new session beside (4 sessions)" || check A4c FAIL "session count $(count), expected 4"
cmd A "status"; wait_for A "Lark turn 1" 20
k A M-s; wait_for A "Sessions" 20; sleep 0.5; snap A a4-list
down_to A "kestrel"; k A Enter; sleep 2; cmd A "status"; wait_for A "Kestrel turn 2" 20; snap A a4-back
expect A4d a4-back "Kestrel turn 2" "Alt+S back: the old conversation still answers"

# Plain nori: /agents is an alias there too.
plain P; sleep 6; cmd P "/agents"; sleep 2; snap P a2-plain-agents
expect A2b a2-plain-agents "Kestrel|Lark" "plain nori /agents opens the agent picker"
teardown
