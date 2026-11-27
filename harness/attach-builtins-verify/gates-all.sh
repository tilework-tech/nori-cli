#!/usr/bin/env bash
H=$(cd "$(dirname "$0")" && pwd); T=$HOME/targets; O=$1/gates
for pair in "802:main" "803:main" "804:main" "main:804" "804:803"; do
  c=${pair%%:*}; h=${pair#*:}; mkdir -p "$O/client-$c-host-$h"
  bash "$H/gates.sh" "$T/$c/debug/nori" "$T/$h/debug/nori" "$O/client-$c-host-$h" g > "$O/client-$c-host-$h/stdout.txt" 2>&1
done
