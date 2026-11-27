#!/usr/bin/env bash
# /mention in attach (nori-cli #804): host vs client folders, session folders, watchers, images.
NORI=$1; OUT=$2; mkdir -p "$OUT"; source "$(dirname "$0")/lib.sh"
setup_host mention kestrel
printf '\x89PNG\r\n\x1a\n synthetic' > "$PROJECT/harbor-buoy.png"
printf '\x89PNG\r\n\x1a\n synthetic' > "$FOLDER/harbor-buoy.png"
mention() { # mention TERM QUERY LABEL
  cmd "$1" "/mention"; sleep 0.6; typed "$1" "$2"; sleep 2; snap "$1" "$3"; }
clear_composer() { k "$1" Escape; sleep 0.3; k "$1" C-u; sleep 0.3; }
term A; wait_for A "Sessions" 40; sleep 0.5; k A Escape; sleep 0.5
typed A "/men"; sleep 0.8; snap A c0-slash; k A C-u
expect C0a c0-slash "/mention" "/mention is offered in attach"
expect_not C0b c0-slash "/mention .*not available" "/mention is not greyed on a current host"
mention A "harbor-chart" c1-no-session
expect C1a c1-no-session "harbor-chart-on-host.md" "no session: host folder file is offered"
expect_not C1b c1-no-session "harbor-chart-on-client.md" "no session: the attaching folder is never searched"
clear_composer A
mark; cmd A "/new"; wait_for A "kestrel options" 30; sleep 1
mention A "harbor-chart" c1-session
expect C1c c1-session "harbor-chart-on-host.md" "in a session: session folder file is offered"
expect_not C1d c1-session "harbor-chart-on-client.md" "in a session: client file not offered"
k A Tab; sleep 1; snap A c2-inserted
expect C2a c2-inserted "〉@?harbor-chart-on-host.md" "Tab inserts the host path"
k A Enter; sleep 3; snap A c2-sent
grep -q "harbor-chart-on-host.md" "$ROOT/quill-kestrel.log" && check C2b PASS "agent receives the mentioned host path" || check C2b FAIL "agent prompt lacks the host path"
# Image match: inserted as a host path, not bytes read on the client
mention A "harbor-buoy" c5-image-list
k A Tab; sleep 1; snap A c5-image-inserted; k A Enter; sleep 3; snap A c5-image-sent
tail -1 "$ROOT/quill-kestrel.log" > "$SC/c5-agent-prompt.txt"
grep -q "blocks=\['text'\]" "$SC/c5-agent-prompt.txt" && grep -q "harbor-buoy.png" "$SC/c5-agent-prompt.txt" \
  && check C5a PASS "image match goes to the agent as a host path, no image bytes" || check C5a FAIL "image prompt: $(cat "$SC/c5-agent-prompt.txt")"
# A session in another host folder: search follows that session's folder
cmd A "/new $OTHER kestrel"; wait_for A "kestrel options" 30; sleep 1
mention A "harbor-chart" c3-other-folder
expect C3a c3-other-folder "harbor-chart-in-other-host-folder.md" "search follows the foreground session's folder"
expect_not C3b c3-other-folder "harbor-chart-on-host.md|harbor-chart-on-client.md" "other folders are not mixed in"
clear_composer A
# Watcher can mention too
SID=$(ids | comm -13 "$ROOT/ids.before" - | head -1)
term W "${SID:0:8}"; sleep 5
mention W "harbor-chart" c4-watcher
expect C4a c4-watcher "harbor-chart-in-other-host-folder.md" "a watcher's /mention searches the watched session's host folder"
clear_composer W
# Plain nori parity (local folder)
plain P; sleep 6; mention P "harbor-chart" c6-plain
expect C6a c6-plain "harbor-chart-on-host.md" "plain nori /mention completes from its own folder"
k P Tab; sleep 1; snap P c6-plain-inserted
expect C6b c6-plain-inserted "〉@?harbor-chart-on-host.md" "plain nori inserts the same way"
teardown
