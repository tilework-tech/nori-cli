#!/bin/bash
# usage: stress.sh LABEL BINARY ATTEMPTS_PER_LOOP
for k in a b c; do python3 -I hunt.py "$2" "$1-$k" "$3" > "evidence/stress-$1-$k.log" 2>&1 & done; wait
