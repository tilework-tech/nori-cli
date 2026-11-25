#!/bin/bash
# #770's own scripts (bench 789a2ce5) rerun independently on this verifier's builds.
set -uo pipefail
B=${BIN:-$HOME/org/workspace/cli/.worktrees/perfwork/bin}
O=$(pwd)/results/769-0e673ef4/linux/bench770
mkdir -p $O
while [ ! -f results/769-0e673ef4/linux/DONE3 ]; do sleep 5; done
S=$HOME/org/workspace/cli/.worktrees/bench/scripts/attach_perf
cd $HOME/org/workspace/cli/.worktrees/bench
python3 $S/replay.py $B/nori-base --turns 64 --sessions 1 --samples 5 --live --stream --output $O/replay-base-64-1.json > $O/replay-base.log 2>&1
python3 $S/replay.py $B/nori-fix --turns 64 --sessions 1 --samples 5 --live --stream --check-batching --output $O/replay-fix-64-1.json > $O/replay-fix.log 2>&1
python3 $S/replay.py $B/nori-fix --turns 64 --samples 1 --live --audit --screen $O/fix-screen-64.txt --output $O/replay-fix-audit.json > $O/replay-fix-audit.log 2>&1
python3 $S/replay.py $B/nori-base --turns 64 --samples 1 --live --audit --screen $O/base-screen-64.txt --output $O/replay-base-audit.json > $O/replay-base-audit.log 2>&1
python3 $S/transport.py --build base $B/nori-base --build fix $B/nori-fix --turns 32 --output $O/transport-32.json > $O/transport.log 2>&1
python3 $S/recovery.py $B/nori-fix --samples 3 --turns 64 --output $O/recovery-fix.json > $O/recovery-fix.log 2>&1
python3 /tmp/k/host_startup_benchmark.py --base $B/nori-base --head $B/nori-fix --samples 20 --output $O/startup-764.json > $O/startup-764.log 2>&1
python3 $S/startup.py --benchmark /tmp/k/host_startup_benchmark.py --binary $B/nori-base --binary $B/nori-fix --samples 5 --output $O/startup-ready.json > $O/startup-ready.log 2>&1
echo DONE > $O/DONE
