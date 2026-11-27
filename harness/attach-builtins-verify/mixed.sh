#!/usr/bin/env bash
# Mixed host contract versions: 1.10 clients on 1.9 hosts and the reverse.
# usage: mixed.sh OUTROOT   (binaries under ~/targets/<name>/debug/nori)
H=$(cd "$(dirname "$0")" && pwd); T=$HOME/targets; O=$1/mixed
run() { # run LABEL CLIENT HOST SCRIPT...
  local L=$1 C=$2 HO=$3; shift 3
  for s in "$@"; do mkdir -p "$O/$L/$s-run"
    HOSTNORI=$T/$HO/debug/nori bash "$H/$s.sh" "$T/$C/debug/nori" "$O/$L/$s-run" > "$O/$L/$s-run/stdout.txt" 2>&1; done
}
# 1.10 client, 1.9 host that already serves the features (interactive path, untagged journal records)
run client-804-7c469a25-host-803-61789f95 804c 803 model mode inplace-mode mention
# 1.10 client, 1.9 host with prepared agents but no #803/#804 host changes
run client-804-7c469a25-host-802-c352fb74 804c 802 agent model mode mention
# 1.10 client, main host (no prepared-agent capability): contract-gated path
mkdir -p "$O/client-804-7c469a25-host-main"; GATED=1 HOSTNORI=$T/main/debug/nori bash "$H/gates.sh" "$T/804c/debug/nori" "$T/main/debug/nori" "$O/client-804-7c469a25-host-main" g > "$O/client-804-7c469a25-host-main/stdout.txt" 2>&1
# Reverse: main client on a 1.10 host
mkdir -p "$O/client-main-host-804-7c469a25"; bash "$H/gates.sh" "$T/main/debug/nori" "$T/804c/debug/nori" "$O/client-main-host-804-7c469a25" g > "$O/client-main-host-804-7c469a25/stdout.txt" 2>&1
# 1.9 client (old #803 head) on a 1.10 host
run client-803-61789f95-host-804-7c469a25 803 804c model mode
