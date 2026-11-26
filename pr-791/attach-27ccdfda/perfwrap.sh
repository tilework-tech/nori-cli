#!/bin/bash
# Profiles only `nori attach` (perf attaches to this PID, which exec keeps); everything else runs the real binary.
REAL=~/org/workspace/.worktrees/m1bin/nori-new1
if [ "$1" = attach ]; then
  setsid /usr/bin/perf record -e cpu-clock -F 4000 --call-graph dwarf,16384 -p $$ -o /tmp/m1perf/attach-$$.data </dev/null >/tmp/m1perf/perf-$$.log 2>&1 &
  sleep 0.3
fi
exec "$REAL" "$@"
