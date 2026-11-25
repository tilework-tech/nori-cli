#!/bin/bash
# F1 / B1-B3 recheck: #769 e2e98ba3 against former head 0e673ef4, stack parent d8c061da and main 80992049.
set -uo pipefail
B=${BIN:-$HOME/org/workspace/cli/.worktrees/perfwork/bin}
A=$HOME/org/workspace/cli/.worktrees/accept769
O=$A/results/769-e2e98ba3/linux
cd $A
mkdir -p $O/aborted; mv $O/recover-* $O/aborted/ 2>/dev/null
sha256sum $B/nori-fix2 $B/nori-fix $B/nori-parent $B/nori-base > $O/SHA256SUMS
H="python3 -I harness/perf/replay769.py"
P3="$H --build fix $B/nori-fix2 --build oldfix $B/nori-fix --build parent $B/nori-parent"
sleep 0; $P3 --mode recover --sections 64 --samples 12 --keep-scrollback --out $O/recover-transcript-64.json > $O/recover-transcript-64.log 2>&1
$P3 --mode recover --native --sections 64 --samples 12 --keep-scrollback --out $O/recover-native-64.json > $O/recover-native-64.log 2>&1
PB="$H --build fix $B/nori-fix2 --build base $B/nori-base"
$PB --mode recover --sections 64 --samples 10 --keep-scrollback --out $O/recover-base-transcript-64.json > $O/recover-base-transcript-64.log 2>&1
$PB --mode recover --native --sections 64 --samples 10 --keep-scrollback --out $O/recover-base-native-64.json > $O/recover-base-native-64.log 2>&1
P2="$H --build fix $B/nori-fix2 --build parent $B/nori-parent"
$P2 --sections 150 --samples 3 --out $O/open-150.json > $O/open-150.log 2>&1
$P2 --mode recover --sections 150 --samples 2 --keep-scrollback --out $O/recover-transcript-150.json > $O/recover-transcript-150.log 2>&1
echo DONE > $O/DONE1
# #770 at 588423e7 from a clean clone
C=${CLEAN:-/tmp/clean770.ZNP5/c}; R=${REL6:-/tmp/rel6.7Ru1/x/nori/nori}
M=$O/bench770; mkdir -p $M; cd $C
python3 -I -m unittest discover -s scripts/attach_perf > $M/unittest.txt 2>&1
for i in $(seq 1 10); do
  for build in oldfix:$B/nori-fix fix:$B/nori-fix2; do
    for mode in transcript native; do
      n=${build%%:*}; bin=${build#*:}
      if python3 scripts/attach_perf/recovery.py $bin --samples 1 --turns 64 --modes $mode --output $M/recovery-$n-$mode-$i.json > $M/recovery-$n-$mode-$i.log 2>&1; then
        echo "$n $mode $i pass" >> $M/recovery-tally.txt
      else
        echo "$n $mode $i FAIL $(grep -E 'Error|assert' $M/recovery-$n-$mode-$i.log | tail -1)" >> $M/recovery-tally.txt
      fi
    done
  done
done
python3 scripts/attach_perf/replay.py $R --turns 64 --sessions 1 --samples 5 --live --stream --output $M/replay-next6-64-1.json > $M/replay-next6.log 2>&1
python3 scripts/attach_perf/replay.py $B/nori-fix2 --turns 64 --sessions 1 --samples 5 --live --stream --check-batching --output $M/replay-fix-64-1.json > $M/replay-fix.log 2>&1
python3 scripts/attach_perf/replay.py $B/nori-fix2 --turns 64 --samples 1 --live --audit --screen $M/fix-screen-64.txt --output $M/replay-fix-audit.json > $M/replay-fix-audit.log 2>&1
echo DONE > $O/DONE2
