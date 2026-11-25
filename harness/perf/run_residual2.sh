#!/bin/bash
set -uo pipefail
W=$HOME/org/workspace/cli/.worktrees
H=$W/accept769/harness/perf
O=$W/accept769/results/769-085dac66/linux
BIN=$W/perfwork/bin
until grep -q ALLDONE $O/PROGRESS 2>/dev/null; do sleep 20; done
bash $H/run_palette.sh
echo palette done >> $O/PROGRESS
cd $H
for k in a b c; do python3 -I stress769.py --build fix $BIN/nori-fix3 --build ctl $BIN/nori-fix2 --attempts 12 --sections 85 --tag lin85-$k --out $O/stress/stress-85-$k.jsonl --failures $O/stress/failures > $O/stress/stress-85-$k.log 2>&1 & done; wait
echo stress85 done >> $O/PROGRESS
echo ALLDONE2 >> $O/PROGRESS
