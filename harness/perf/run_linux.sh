#!/bin/bash
# Linux matrix for #769: base main 80992049 vs fix 0e673ef4, rotating order.
set -uo pipefail
B=${BIN:-$HOME/org/workspace/cli/.worktrees/perfwork/bin}
O=${OUT:-results/769-0e673ef4/linux}
P="python3 -I harness/perf/replay769.py --build base $B/nori-base --build fix $B/nori-fix"
$P --sections 64 --samples 5 --out $O/open-64.json > $O/open-64.log 2>&1
$P --sections 64 --extra-rows 24 --cols 160 --rows 50 --samples 3 --out $O/open-64-rows24-160x50.json > $O/open-64-rows24.log 2>&1
$P --sections 4 --samples 3 --out $O/open-4.json > $O/open-4.log 2>&1
$P --sections 256 --samples 3 --out $O/open-256.json > $O/open-256.log 2>&1
$P --mode recover --sections 64 --samples 3 --keep-scrollback --out $O/recover-transcript-64.json > $O/recover-transcript-64.log 2>&1
$P --mode recover --native --sections 64 --samples 3 --keep-scrollback --out $O/recover-native-64.json > $O/recover-native-64.log 2>&1
echo DONE > $O/DONE
