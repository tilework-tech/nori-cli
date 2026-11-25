#!/bin/bash
set -uo pipefail
B=${BIN:-$HOME/org/workspace/cli/.worktrees/perfwork/bin}
O=${OUT:-results/769-0e673ef4/linux}
while [ ! -f $O/DONE ]; do sleep 5; done
P="python3 -I harness/perf/replay769.py --build base $B/nori-base --build fix $B/nori-fix"
$P --mode recover --sections 64 --samples 5 --keep-scrollback --out $O/recover-transcript-64-repro.json > $O/recover-transcript-64-repro.log 2>&1
$P --sections 150 --samples 3 --out $O/open-150.json > $O/open-150.log 2>&1
python3 -I harness/perf/replay769.py --build fix $B/nori-fix --sections 256 --samples 1 --out $O/open-256-fix.json > $O/open-256-fix.log 2>&1
echo DONE > $O/DONE2
