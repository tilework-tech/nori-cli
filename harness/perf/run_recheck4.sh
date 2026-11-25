cd /tmp/clean770.ZNP5/c
for i in $(seq 7 16); do for build in fix:/home/sprite/org/workspace/cli/.worktrees/perfwork/bin/nori-fix2 parent:/home/sprite/org/workspace/cli/.worktrees/perfwork/bin/nori-parent oldfix:/home/sprite/org/workspace/cli/.worktrees/perfwork/bin/nori-fix; do
  n=${build%%:*}; bin=${build#*:}
  [ $n != fix ] && [ $i -gt 11 ] && continue
  if python3 scripts/attach_perf/recovery.py $bin --samples 1 --turns 160 --modes transcript --output /home/sprite/org/workspace/cli/.worktrees/accept769/results/769-e2e98ba3/linux/bench770/recovery-$n-t160-$i.json > /home/sprite/org/workspace/cli/.worktrees/accept769/results/769-e2e98ba3/linux/bench770/recovery-$n-t160-$i.log 2>&1; then r=pass; else r="FAIL $(grep -E 'Error|assert' /home/sprite/org/workspace/cli/.worktrees/accept769/results/769-e2e98ba3/linux/bench770/recovery-$n-t160-$i.log | tail -1)"; fi
  echo "$n transcript turns=160 $i $r" >> /home/sprite/org/workspace/cli/.worktrees/accept769/results/769-e2e98ba3/linux/bench770/recovery-long-tally.txt
done; done
echo DONE > /home/sprite/org/workspace/cli/.worktrees/accept769/results/769-e2e98ba3/linux/bench770/DONE-t160
