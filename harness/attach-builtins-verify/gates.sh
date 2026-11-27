#!/usr/bin/env bash
# Mixed versions. usage: gates.sh CLIENT_BIN HOST_BIN OUT LABEL
NORI=$1; HOSTNORI=$2; OUT=$3; L=$4; mkdir -p "$OUT"; source "$(dirname "$0")/lib.sh"
setup_host "$L" kestrel
term A; wait_for A "Sessions" 40; sleep 0.5; k A Escape; sleep 0.5
for c in age mod con men; do typed A "/$c"; sleep 0.8; snap A "g-slash-$c"; k A C-u; sleep 0.3; done
cmd A "/agents"; sleep 1.5; snap A g-agents-no-session; k A Escape; sleep 0.3; k A C-u
typed A "@harbor-chart"; sleep 2; snap A g-at-no-session; k A C-u; k A Escape; sleep 0.3; k A C-u
cmd A "/new"; wait_for A "kestrel options" 30; sleep 1
typed A "@harbor-chart"; sleep 2; snap A g-at-session; k A C-u; k A Escape; sleep 0.3; k A C-u
cmd A "/model"; sleep 1.5; snap A g-model-session; k A Escape; sleep 0.3; k A C-u
cmd A "status"; wait_for A "Kestrel turn" 20; snap A g-prompt-delivered
teardown
