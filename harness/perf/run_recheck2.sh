#!/bin/bash
# Residual-rate estimate for F1 on e2e98ba3 alone, then a real-terminal capture of it.
set -uo pipefail
B=${BIN:-$HOME/org/workspace/cli/.worktrees/perfwork/bin}
A=$HOME/org/workspace/cli/.worktrees/accept769
O=$A/results/769-e2e98ba3/linux
cd $A
while [ ! -f $O/DONE2 ]; do sleep 10; done
H="python3 -I harness/perf/replay769.py --build fix $B/nori-fix2"
$H --mode recover --sections 64 --samples 24 --keep-scrollback --out $O/recover-fixonly-transcript-64.json > $O/recover-fixonly-transcript-64.log 2>&1
$H --mode recover --native --sections 64 --samples 16 --keep-scrollback --out $O/recover-fixonly-native-64.json > $O/recover-fixonly-native-64.log 2>&1
mkdir -p $O/captures
python3 -I harness/perf/capture_split.py $B/nori-fix2 $O/captures/fix-e2e98ba3-recovery-notice 60 > $O/captures/capture.log 2>&1
echo DONE > $O/DONE3
