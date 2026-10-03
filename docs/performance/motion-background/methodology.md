# Motion profiling methodology

This is a baseline investigation of the stripped `motion_storybook`, before
optimizing field geometry, sampling, colors, or terminal output. The example
runs only Dots ↔ Formations (three seconds each way), on the whole screen.
Two policies pin the destination to Platoon or Muster; the default public
component still uses the ambient cycle. No former login/card UI is rendered.

## Measurements

- **Headless:** real widget → Ratatui lazy buffer diff → Crossterm encoding into
  a byte-counting sink → buffer reset. No test backend, simplified field model,
  mocked encoder, OS output, sleeping, or terminal parser. A separate diff-only
  scan is a warm-cache diagnostic, excluded from pipeline total. It must not be
  treated as an exact additive split of the fused diff/encoder pass.
- **Live:** actual `Terminal<CrosstermBackend<Stdout>>::draw` through a PTY into
  isolated tmux, recording widget time, total draw time, the remainder outside
  the widget, and start-to-start intervals. The remainder includes terminal
  size queries, buffer diff, ANSI encoding, write/flush, cursor handling, and
  buffer reset. Timing is accumulated in memory and saved only after alternate
  screen restoration. No per-cell timing is added to the renderer.
- **Planned CPU sampling (interrupted; no retained profile here):** optimized build with release optimization/LTO unchanged,
  `debug=line-tables-only`, `strip=none`, using Linux perf userspace cycles and
  DWARF call stacks. Instruction samples identify hot functions/source lines;
  they are separate runs, not mixed into the wall-clock timing data.

A completed write is not proof that a graphical terminal displayed that frame.
The live results measure production throughput and tmux/PTY backpressure, not
monitor refresh, compositor presentation, or the user's terminal's displayed FPS.
The headless sink's Write-call count is not a system-call count.

## Sampling protocol

All cases run serially after builds/tests finish, on the same shared host, with
`NO_COLOR` unset, `TERM=xterm-256color`, `COLORTERM=truecolor`. No affinity,
priority, power policy, terminal setting, or system security setting is changed.

The headless matrix has debug/release × 80×24/120×40/200×60/320×100 ×
Platoon/Muster × three repetitions. Each run warms a full six-second loop, then
records 300 deterministic frames at a simulated 25 FPS (two loops). Case order
is shuffled with seed 926. The first measured full repaint is excluded from
summaries, leaving 897 frames per configuration. The clock resets only between
runs, not at zoom reversals.

The live matrix records 300 frames per case at 25 FPS for debug/release ×
120×40/200×60/320×100 × both variants. Additional release 200×60 cases request
60 FPS. Geometry advances by actual elapsed time, preserving the original
start-to-start scheduler and its behavior under overload. As a consequence,
heavy phases produce fewer frames when late; headless and live percentile
populations are not identical. First live frames are excluded from summaries.

The report uses nearest-rank percentiles. `work_over_budget_pct` compares work
time with 1/FPS. `intervals_over_1_5_budget_pct` counts conspicuously long
start-to-start gaps, allowing small scheduler jitter. Neither is claimed to be
a measured count of display-dropped frames. `produced_fps` is the reciprocal of
mean start-to-start interval. Phase bands are early [0,.45), middle [.45,.75),
and late [.75,1].

## Reproduce

From `cli/nori-rs`, build both example profiles first, then run without Cargo in
the timed region:

```sh
cargo build -p nori-tui-components --example motion_storybook
CARGO_PROFILE_RELEASE_DEBUG=line-tables-only CARGO_PROFILE_RELEASE_STRIP=none \
  cargo build --release -p nori-tui-components --example motion_storybook

env -u NO_COLOR TERM=xterm-256color COLORTERM=truecolor \
  target/release/examples/motion_storybook --bench --size 200x60 \
  --frames 300 --profile /tmp/platoon.csv

env -u NO_COLOR TERM=xterm-256color COLORTERM=truecolor \
  target/release/examples/motion_storybook --bench --size 200x60 \
  --muster --frames 300 --profile /tmp/muster.csv

# In the terminal you want to evaluate, sized as desired:
env -u NO_COLOR TERM=xterm-256color COLORTERM=truecolor \
  target/release/examples/motion_storybook --muster --frames 300 \
  --profile /tmp/muster-live.csv

python3 tui-components/scripts/summarize-motion-profile.py \
  /tmp/platoon.csv /tmp/muster.csv /tmp/muster-live.csv --output /tmp/summary
```

The summary tool also accepts `.csv.gz` inputs. See [measurement status](report.md)
for which artifacts survived the interruption.
