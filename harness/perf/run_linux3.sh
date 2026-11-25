#!/bin/bash
set -uo pipefail
B=${BIN:-$HOME/org/workspace/cli/.worktrees/perfwork/bin}
O=${OUT:-results/769-0e673ef4/linux}
while [ ! -f $O/DONE2 ]; do sleep 5; done
P="python3 -I harness/perf/replay769.py --build base $B/nori-base --build fix $B/nori-fix"
$P --mode recover --sections 64 --samples 6 --keep-scrollback --out $O/recover-transcript-64-instrumented.json > $O/recover-transcript-64-instrumented.log 2>&1
$P --mode recover --native --sections 64 --samples 6 --keep-scrollback --out $O/recover-native-64-instrumented.json > $O/recover-native-64-instrumented.log 2>&1
$P --sections 64 --samples 3 --out $O/open-64-instrumented.json > $O/open-64-instrumented.log 2>&1
echo DONE > $O/DONE3
