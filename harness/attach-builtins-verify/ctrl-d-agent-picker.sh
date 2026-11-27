#!/usr/bin/env bash
# Ctrl+D inside the attach agent picker, then after Esc. usage: ctrl-d-agent-picker.sh BIN LABEL (writes /tmp/cd-LABEL)
NORI=$1; OUT=/tmp/cd-$2; rm -rf $OUT; mkdir -p $OUT; source ./lib.sh
setup_host cd kestrel
term A; wait_for A "Sessions" 40; k A Escape; sleep 0.5
cmd A "/agents"; wait_for A "Choose an agent" 20; k A C-d; sleep 2; echo "$2 ctrl+d in agent picker -> $(screen A | grep -c EXITED) exited, picker=$(screen A | grep -c 'Choose an agent')"
k A Escape; sleep 0.5; k A C-d; sleep 2; echo "$2 esc then ctrl+d -> $(screen A | grep -c EXITED) exited"
teardown
