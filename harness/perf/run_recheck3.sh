#!/bin/bash
# Detection power of #770's recovery audit on longer fixtures, a large in-bounds recovery, and the shipped musl baseline.
set -uo pipefail
B=${BIN:-$HOME/org/workspace/cli/.worktrees/perfwork/bin}
A=$HOME/org/workspace/cli/.worktrees/accept769
O=$A/results/769-e2e98ba3/linux
cd $A
while [ ! -f $O/DONE3 ]; do sleep 10; done
python3 -I harness/perf/replay769.py --build fix $B/nori-fix2 --build oldfix $B/nori-fix --mode recover --sections 85 --samples 4 --keep-scrollback --out $O/recover-transcript-85.json > $O/recover-transcript-85.log 2>&1
C=${CLEAN:-/tmp/clean770.ZNP5/c}; R=${REL6:-/tmp/rel6.7Ru1/x/nori/nori}
M=$O/bench770; cd $C
for turns in 128 160; do
  for i in 1 2 3 4 5 6; do
    for build in oldfix:$B/nori-fix fix:$B/nori-fix2; do
      n=${build%%:*}; bin=${build#*:}
      if python3 scripts/attach_perf/recovery.py $bin --samples 1 --turns $turns --modes transcript --output $M/recovery-$n-t$turns-$i.json > $M/recovery-$n-t$turns-$i.log 2>&1; then
        echo "$n transcript turns=$turns $i pass" >> $M/recovery-long-tally.txt
      else
        echo "$n transcript turns=$turns $i FAIL $(grep -E 'Error|assert' $M/recovery-$n-t$turns-$i.log | tail -1)" >> $M/recovery-long-tally.txt
      fi
    done
  done
done
python3 scripts/attach_perf/replay.py $R --turns 64 --sessions 1 --samples 5 --live --stream --output $M/replay-next6-64-1.json > $M/replay-next6.log 2>&1
echo DONE > $O/DONE4
