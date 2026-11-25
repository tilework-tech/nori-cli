#!/bin/bash
set -uo pipefail
W=$HOME/org/workspace/cli/.worktrees
O=$W/accept769/results/769-085dac66/linux/palette773
BIN=$W/perfwork/bin
mkdir -p $O
cd $W/accept769/harness/perf
python3 -I palette773.py --build base $BIN/nori-fix3 --build fix773 $BIN/nori-pal --samples 5 --out $O/palette773.json > $O/palette773.log 2>&1
cd $W/pal
git status --porcelain > $O/measure-checkout.txt; git rev-parse HEAD >> $O/measure-checkout.txt
python3 scripts/terminal-palette/measure.py --fixtures scripts/attach_perf --base $BIN/nori-fix3 --binary $BIN/nori-pal --paths attach resume host-ls --samples 3 --check --output $O/measure-pr773.json > $O/measure-pr773.log 2>&1; echo "measure exit $?" >> $O/exit.txt
echo DONE >> $O/exit.txt
