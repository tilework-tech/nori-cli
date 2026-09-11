# Motion: baseline and straightforward optimizations

Measured September 11, 2026, against commit `549032f3`. The final headless
comparison interleaves the preserved baseline and optimized release binaries.
P95 frame work fell **21–26%**, with the same sampling density, animation timing,
colors, and geometry. The measured frame/style fingerprints and ANSI byte counts
match. Large live terminal frames still sometimes exceed the 25 FPS budget.

## Headless results

Real widget → Ratatui lazy diff → Crossterm ANSI encoder → buffer reset. No
terminal I/O or sleeping. The sink's writes are method calls, not syscalls.

| Viewport / variant | Baseline p95 | Optimized p95 | Reduction |
|---|---:|---:|---:|
| 120x40 platoon | 5.65 ms | 4.47 ms | 20.9% |
| 120x40 muster | 5.89 ms | 4.41 ms | 25.2% |
| 240x80 platoon | 22.73 ms | 17.89 ms | 21.3% |
| 240x80 muster | 23.48 ms | 17.60 ms | 25.0% |
| 320x100 platoon | 37.92 ms | 29.88 ms | 21.2% |
| 320x100 muster | 38.90 ms | 28.80 ms | 26.0% |

Use these interleaved results for the primary comparison. Each entry contains
298 measured frames (two repetitions of a six-second loop, excluding the first
full repaint of each repetition). P95/p99, stage timings, maxima, and budget
counts are in [summary.csv](summary.csv); phase-specific timings are in
[phases.csv](phases.csv). Do not add individual stage percentiles together.

The earlier sequential baseline had 320x100 p95 of 41.78/44.10 ms, with 9.4/14.3%
of frames above 40 ms. A later unchanged-binary check ran faster. That host drift
is why the more conservative interleaved comparison above is the headline,
rather than attributing all of the initial 28–36% difference to code changes.

## Actual terminal production

Finite runs through a PTY into detached tmux, target 25 FPS. Two repetitions per
120x40/240x80 case; one per 320x100 case. These are different elapsed-time sample
populations: overloaded live runs advance their animation by real time. They do
not present the same fixed sequence as the headless test, and fewer heavy frames
are produced when deadlines are missed.

| Viewport / variant | Baseline p95 draw | Optimized p95 draw | Produced FPS before → after |
|---|---:|---:|---:|
| 120x40 platoon | 20.79 ms | 18.34 ms | 24.63 → 24.63 |
| 120x40 muster | 23.41 ms | 18.88 ms | 24.65 → 24.63 |
| 240x80 platoon | 42.48 ms | 37.54 ms | 24.27 → 24.52 |
| 240x80 muster | 39.33 ms | 37.57 ms | 24.55 → 24.56 |
| 320x100 platoon | 51.23 ms | 47.58 ms | 23.62 → 24.25 |
| 320x100 muster | 58.61 ms | 45.66 ms | 22.95 → 24.02 |

At 320x100, work above 40 ms fell from 26.8% to 13.4% for platoon and 24.2% to
15.4% for muster in these limited live samples. This is useful improvement, but
**not a complete stutter fix or a 60 FPS result**. Producer throughput is not
monitor/compositor presentation; no graphical-terminal displayed FPS was measured.

The output residual did not get uniformly faster: its p95 rose in these live
runs, despite substantially fewer write calls. Buffer bursts, PTY backpressure,
CPU power behavior, and changed wall-clock trajectories can affect that number;
this experiment does not isolate their contributions. In particular, fewer
syscalls must not be reported as an equivalent FPS gain.

Separate strace runs at 240x80 measured `write` calls across the finite process:

| Variant | Baseline calls | Optimized calls | Reduction |
|---|---:|---:|---:|
| Platoon | 34,119 | 626 | 98.2% |
| Muster | 33,957 | 629 | 98.1% |

Live and strace runs used the same optimized logic before final formatting;
their binary hashes are recorded separately from the final headless run.
Counts include setup, restoration, and CSV saving. Traced runs are excluded
from wall-clock summaries; tracing perturbs scheduling and syscall costs.
Their timing CSVs are retained for provenance, not performance conclusions.

## Changes kept

- Prepare the existing quantized palette once per frame; preserve custom RGB,
  terminal colors, modifiers, and the exact non-RGB dimming threshold.
- Cache island drift/pulse by tile within a frame (64 direct-mapped entries).
- Cache noise lattice corners (16 entries) and time interpolation weight;
  retain multiply/add order for interpolated noise.
- Cache four muster rows, including their logarithmic depth coordinate.
- Calculate zoom blend, braille blend, formation selection/fade, and scene fade
  once per frame.
- Skip invisible dot geometry at the formation endpoint and ambient noise under
  an opaque pattern. Soft formation fades still compute both contributing fields.
- Hold stdout's lock and use a 64 KiB BufWriter in the shared storybook terminal.
  Ratatui already performed incremental updates; it still owns the previous/current
  buffers, diffing, and Crossterm encoding. No custom ANSI renderer was added.

Caches are bounded and frame-local, with coordinate-checked entries. Collisions
recompute values; no cross-frame invalidation, persistent scene graph, or caller
cache API is needed. Rendering a clipped region cannot change its field origin.
The reusable widget remains terminal-independent. Applications adopting it must
choose their own backend writer; the buffering change currently serves storybooks.

## Verification

All 1,160 cell/style fingerprints match the baseline. The equivalence utility
covers every sampled frame of both zoom directions at 36x20 and 120x40, all five
scenes at several ambient times and transition positions, ASCII and RGB/terminal
policies, clipping, and quiet regions. This is finite regression coverage, not a
proof for every floating-point coordinate or font.

All existing snapshots match without updates. Additional tests cover noise-cache
collisions/negative coordinates, prepared custom palettes, and clipped/cold-cache
zoom equivalence. The crate suite and all opt-in storybook terminal tests pass,
including the other examples affected by buffered terminal support.

## Artifacts and reproduction

- `baseline/`: initial headless matrix (three repetitions) and live baseline.
- `candidate/`: intermediate caching pass, before skipping hidden ambient work.
- `optimized/`: combined headless/live run.
- `baseline-check/`: later unchanged-binary headless check for host drift.
- `paired-baseline/`, `paired-optimized/`: primary interleaved comparison of the
  final formatted release binary. `preformat-*` retains the earlier check;
  formatting changed debug metadata, and the final comparison was rerun.
- `baseline-large/`, `optimized-large/`: 320x100 terminal runs.
- `trace-baseline/`, `trace-optimized/`: separate syscall runs.
- `*-frames.txt`: fingerprints before/after; metadata JSONs contain exact commands
  and binary SHA256 hashes. `environment.json` records build/host information.
- Raw per-frame CSVs are gzip-compressed after measurement. Each finite run saved
  its data immediately to this directory; nothing depends on temporary storage.

Build the baseline commit and candidate in separate checkouts. Use Rust 1.90.0,
release optimization with fat LTO and one codegen unit, debug line tables, no
stripping, and the same linker/flags. Build before measuring. On this host:

```sh
CARGO_BUILD_JOBS=2 CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=cc RUSTFLAGS='' \
CARGO_PROFILE_RELEASE_DEBUG=line-tables-only CARGO_PROFILE_RELEASE_STRIP=none \
cargo build --release -p nori-tui-components --example motion_storybook --example motion_equivalence
```

`motion_equivalence` is introduced by this change: copy its unchanged example
source into the baseline checkout before building that utility. Run it from each build and compare its stdout with `cmp`.
For interleaved timings, supply the two built binaries and a fresh output path:

```sh
python3 docs/performance/motion-low-hanging/paired.py BASELINE_BINARY OPTIMIZED_BINARY /tmp/new-motion-pair
```

`run.py BINARY NEW_LABEL` records the sequential matrix; add `--live --repeats 2`
for finite terminal tests, `--sizes 320x100` to select a size, or `--trace` with
`--live` for syscall counts. Live mode uses only the isolated TUI skill scripts;
`TUI_PUPPETEERING_DIR` can override their location. Existing artifacts are never
overwritten. `summarize.py` reads raw or compressed CSVs in this report directory.

All runs are sequential on a shared host with NO_COLOR unset and true-color
terminal settings. No CPU affinity, power policy, security settings, or frame
quality was changed. The sample size and shared host limit precision; repeated
per-run values and raw traces are retained for reassessment.
