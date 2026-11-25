#!/bin/bash
cd /home/sprite/org/workspace/cli/.worktrees/accept769/harness/perf
for s in 120 130 140; do python3 -I stress769.py --build fix /home/sprite/org/workspace/cli/.worktrees/perfwork/bin/nori-fix3 --attempts 4 --sections $s --tag bound$s --out /home/sprite/org/workspace/cli/.worktrees/accept769/results/769-085dac66/linux/boundary/recover-$s.jsonl --failures /home/sprite/org/workspace/cli/.worktrees/accept769/results/769-085dac66/linux/boundary/failures > /home/sprite/org/workspace/cli/.worktrees/accept769/results/769-085dac66/linux/boundary/recover-$s.log 2>&1; done
python3 -I replay769.py --build fix /home/sprite/org/workspace/cli/.worktrees/perfwork/bin/nori-fix3 --mode recover --sections 64 --samples 1 --out /home/sprite/org/workspace/cli/.worktrees/accept769/results/769-085dac66/linux/captures-fix-recover.json > /dev/null 2>&1
echo ALLDONE3 >> /home/sprite/org/workspace/cli/.worktrees/accept769/results/769-085dac66/linux/PROGRESS
