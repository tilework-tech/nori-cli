# Bare zoom storybook

The example paints only the background: scene 1 (dots) zooms into scene 2
(braille formations) for three seconds, then reverses for three seconds,
continuously. There are no text overlays, cards, menus, headers, or footers.

From `nori-rs/`:

```console
cargo run --release -p nori-tui-components --example motion_storybook
cargo run --release -p nori-tui-components --example motion_storybook -- --muster
```

The default destination is the moving platoon formation. `--muster` pins the
perspective muster instead of waiting for the ambient formation cycle. The
animation clock continues through each reversal. Geometry and sampling formulas
are unchanged by this profiling pass.

Hidden development keys: Space pauses, `s` freezes a repeatable 80%-zoom frame,
and `q`, Escape, or Ctrl-C quits. `--still` starts paused at the dot endpoint.
The default cadence remains 25 FPS; `--fps 60` changes the requested cadence.

## Profile

Live timing uses the actual terminal size and writes CSV only after leaving the
alternate screen. `--frames` makes the run finite. The example does not display
timing counters over the animation.

```console
cargo run --release -p nori-tui-components --example motion_storybook -- --muster --frames 750 --profile /tmp/motion-live.csv
```

For a repeatable CPU/encoder baseline without terminal I/O:

```console
cargo run --release -p nori-tui-components --example motion_storybook -- --bench --size 200x60 --frames 300 --profile /tmp/motion-bench.csv
python3 tui-components/scripts/summarize-motion-profile.py /tmp/motion-live.csv /tmp/motion-bench.csv --output /tmp/motion-summary
```

Unset `NO_COLOR` and use `TERM=xterm-256color COLORTERM=truecolor` when comparing
colored terminal output. Build before collecting measurements and run cases
serially. Repeat with a debug build, both variants, and representative sizes.

The benchmark warms one six-second loop, then measures a deterministic sequence
at the requested FPS without sleeping. It times widget rendering, the real lazy
Ratatui diff plus Crossterm ANSI encoding, and the inactive-buffer reset. It
counts changed cells, ANSI bytes, and calls to the sink's `Write` implementation
(these are **not** system calls). A separate warm-cache diff scan is recorded as
`diff_probe_us` and excluded from `total_us`; do not subtract it and label the
remainder an exact serialization measurement.

Live `output_us` is everything in `Terminal::draw` outside the widget: resize
checks, diff, ANSI formatting, output writes/flush, cursor handling, and buffer
reset. `interval_us` is the actual start-to-start interval. These measure frame
production and PTY backpressure, not the graphical terminal's displayed FPS.
The summary excludes the first full repaint of each run, reports percentiles
and work over the requested frame budget, and separates three zoom-phase bands.

[Measurements and recommendations](../../../../docs/performance/motion-background/report.md)
include the collected data and CPU profile. The component's broader API remains
in the [integration guide](../../../../docs/reference/motion-background.md).
