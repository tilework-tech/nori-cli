#!/usr/bin/env bash
# A watcher on a session that changes agent in place while the host has another live session.
# usage: watcher-two-sessions.sh BIN OUT
NORI=$1; OUT=$2; mkdir -p "$OUT"; source "$(dirname "$0")/lib.sh"
pick() { cmd "$1" "$2"; sleep 1.5; down_to "$1" "$3"; k "$1" Enter; }
setup_host watcher kestrel
term A; wait_for A "Sessions" 40; k A Escape; sleep 0.5
cmd A "/new lark"; wait_for A "lark options" 30; sleep 1; cmd A "status"; wait_for A "Lark turn 1" 20
mark; cmd A "/new kestrel"; wait_for A "kestrel options" 30; sleep 1; K=$(new_sid)
term B "${K:0:8}"; sleep 4; snap B w0-watcher-before
expect W0 w0-watcher-before "watching .* kestrel" "watcher opens the Kestrel session"
pick A "/agent" "Lark"; wait_for A "now runs Lark" 20; closed A
cmd A "first after switch"; wait_for A "Lark read: first after switch" 20
cmd A "second after switch"; wait_for A "Lark read: second after switch" 20
sleep 6; snap B w1-watcher-after; snap A w1-controller-after
expect W1 w1-watcher-after "Lark read: second after switch" "watcher stays on the changed session and sees its new turns"
expect_not W2 w1-watcher-after "Lark turn 1" "watcher is not moved to the other Lark session"
hostcmd ls > "$SC/host-ls.txt"
teardown
