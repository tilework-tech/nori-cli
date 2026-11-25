#!/bin/bash
# Linux recheck of #769 085dac66 vs control e2e98ba3, #770 5705df90 audit, #773 d3165b1c smoke.
set -uo pipefail
W=$HOME/org/workspace/cli/.worktrees
H=$W/accept769/harness/perf
O=$W/accept769/results/769-085dac66/linux
BIN=$W/perfwork/bin
FIX=$BIN/nori-fix3; CTL=$BIN/nori-fix2; PARENT=$BIN/nori-parent; PAL=$BIN/nori-pal
mkdir -p $O/stress $O/open $O/bench770 $O/boundary $O/palette773
sha256sum $FIX $CTL $PARENT $PAL > $O/SHA256SUMS
cat $BIN/nori-fix3.sha $BIN/nori-fix2.sha $BIN/nori-parent.sha $BIN/nori-pal.sha > $O/BUILD_SHAS
cd $H
# 1. three parallel labelled loops, 64 sections
for k in a b c; do python3 -I stress769.py --build fix $FIX --build ctl $CTL --attempts 40 --sections 64 --tag lin64-$k --out $O/stress/stress-64-$k.jsonl --failures $O/stress/failures > $O/stress/stress-64-$k.log 2>&1 & done; wait
echo stress64 done >> $O/PROGRESS
# 2. long labelled recoveries, one loop
python3 -I stress769.py --build fix $FIX --build ctl $CTL --attempts 8 --sections 160 --tag lin160 --out $O/stress/stress-160.jsonl --failures $O/stress/failures > $O/stress/stress-160.log 2>&1
echo stress160 done >> $O/PROGRESS
# 3. #770 audit from its clean checkout
cd $W/bench3
git status --porcelain > $O/bench770/checkout-status.txt; git rev-parse HEAD >> $O/bench770/checkout-status.txt
python3 -m unittest discover -s scripts/attach_perf > $O/bench770/unittest.txt 2>&1
for b in fix:$FIX ctl:$CTL; do n=${b%%:*}; python3 scripts/attach_perf/recovery.py ${b#*:} --samples 8 --turns 128 160 --audit-all --output $O/bench770/long-$n.json > $O/bench770/long-$n.log 2>&1; echo "$n exit $?" >> $O/bench770/exit.txt; done
echo bench770 done >> $O/PROGRESS
# 4. normal long open, unloaded
cd $H
python3 -I replay769.py --build fix $FIX --build ctl $CTL --sections 150 --samples 3 --out $O/open/open-150.json > $O/open/open-150.log 2>&1
python3 -I replay769.py --build fix $FIX --build parent $PARENT --sections 150 --samples 1 --out $O/open/open-150-parent.json > $O/open/open-150-parent.log 2>&1
echo open done >> $O/PROGRESS
# 5. retained-history boundary (fix only)
cd $W/bench3
python3 scripts/attach_perf/recovery.py $FIX --samples 2 --turns 200 216 225 --audit-all --output $O/boundary/recovery-770fixture.json > $O/boundary/recovery-770fixture.log 2>&1; echo "exit $?" >> $O/boundary/exit.txt
cd $H
for s in 170 185 190 200; do python3 -I replay769.py --build fix $FIX --sections $s --samples 1 --out $O/boundary/open-$s.json > $O/boundary/open-$s.log 2>&1; echo "open $s exit $?" >> $O/boundary/exit.txt; done
for s in 90 100 110; do python3 -I stress769.py --build fix $FIX --attempts 2 --sections $s --tag bound$s --out $O/boundary/recover-$s.jsonl --failures $O/boundary/failures > $O/boundary/recover-$s.log 2>&1; done
echo boundary done >> $O/PROGRESS
echo ALLDONE >> $O/PROGRESS
