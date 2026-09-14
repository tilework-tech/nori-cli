# Comparison of the two uncommitted motion implementations

Measured September 10, 2026. Both checkouts have base commit
`fd92abe23e6c94c66ea6ef7afa94678895ecc8f7`; their motion implementations are
uncommitted additions. No component code was changed for this comparison.

Current: `/home/clifford/Documents/source/nori/cli`.
Other: `/home/clifford/.codex/worktrees/b08b/nori/cli/.worktrees/animated-onboarding`,
branch `codex/animated-onboarding`.

## Measurements

Run `python3 run.py` from any directory to reproduce against the existing release
binaries. It runs sequentially, shuffling 16 cases with seed 910: two sizes,
two variants, two implementations, two repetitions. Each process records 150
frames at simulated 25 FPS, excluding its first measured frame from summaries.
There is no live terminal, CPU sampling, rebuild, or concurrent benchmark.
Raw CSVs, the summary, exact commands, and binary SHA256 hashes are saved here.

| Size / variant | Current median / p95 ms | Worktree median / p95 ms |
|---|---:|---:|
| 120x40 default | 0.86 / 5.83 | 3.42 / 6.86 |
| 120x40 muster | 0.91 / 6.38 | 3.97 / 8.16 |
| 240x80 default | 3.92 / 24.67 | 14.01 / 29.59 |
| 240x80 muster | 3.04 / 23.31 | 15.38 / 28.21 |

These are headless widget + buffer diff + ANSI encoding + buffer reset timings,
not displayed FPS. No retained frame exceeded 40 ms in this small run. This does
not supersede the earlier live results or prove either version is smooth.
Absolute timings differ substantially from the earlier runs; compare the two
implementations within this run rather than treating the difference from old
numbers as a code improvement. There were no new rendering optimizations.

This is a comparison of the existing examples, not identical visual work:

- Current pins Platoon or Muster at ambient time 12s. Other starts at time 0s
  (quiet field) or 94s (muster), retaining its ambient cycle.
- Their noise/hash, muster shape, palettes, and sampling schedules differ.
- Current warms a full loop. Other warms ten copies of its initial frame.
- Current streams the actual lazy diff to Crossterm. Other collects the diff
  into a new Vec to time diff and encoding independently, adding allocation
  and separating operations that are fused in live rendering. Widget-only
  p95 is also lower in current, so the harness difference is not the whole gap.
- Build provenance was not normalized by rebuilding; these are existing release
  binaries, with hashes recorded. Small sample count and shared-host scheduling
  limit precision.

Current generated approximately 13–18% more ANSI bytes per frame in these cases.
Its finer palette and different pictures make that unsurprising, but the exact
cause was not isolated. A real terminal may favor the other implementation
more than these headless timings suggest.

## Design assessment

Both correctly leave terminal ownership, events, authentication, and scheduling
outside the reusable widget, and both have determinism/clipping/snapshot tests.
Neither creates a scene graph of heap-allocated objects per field sample.

The worktree has the cleaner minimal animation interface: phase, explicit time,
and transition progress. Callers can select duration, easing, and seek directly.
It also separates zoom/formation geometry from tile/creature geometry. Its
four precomputed styles are simpler and cheaper than current's per-cell RGB
mixing. At the fully formed endpoint it skips the now-invisible island field;
current still calculates that field before blending with weight one.

Current is more complete for integration: interruptible caller-held state,
custom theme-derived palette, an optional quiet region that avoids sampling
under a panel, ASCII fallback, and an explicit fixed formation selector. Its
Field object already computes scale and shape once per frame, where the other
source expresses zoom invariants inside the per-cell function. Current's
noise interpolation weights are outside the eight-corner loop. Compiler
optimization can affect the realized benefit of these source differences.

Current's mandatory MotionState also hardcodes three seconds per adjacent phase
and exposes no direct seek/easing control. That is less flexible as the lowest
level component API. It is better treated as an optional controller over a
renderer accepting explicit time and transition position.

The largest obvious workload difference is braille selection: at zoom 0.5,
current takes one sample per cell while the other takes eight. At zoom 0.75,
current averages 4.94 versus eight (over the Bayer pattern). Consequently,
current's much lower median is partly a different sampling/visual policy,
not a demonstration of an equivalently detailed renderer being four times faster.

The worktree has stronger retained profiling evidence: compressed frame traces,
CPU profiles for quiet/muster/live runs, and syscall counts. Its separate
geometry mode compiles the actual geometry source. Current has more viewport
and FPS controls and a closer-to-live lazy-diff benchmark, but its earlier
raw traces were in temporary storage and are unavailable after the interruption.
These new comparison traces are persisted in the repository directory.

## Recommendation

Use current's frame context and integration capabilities, adopt the other
version's explicit-time/progress rendering API, and retain state transitions as
an optional helper. Preserve the geometry module split and endpoint early-out.
Choose one desired visual/sampling schedule before comparing optimizations.
Then measure frame/tile invariant reuse and output batching independently.
Neither existing branch resolves the large-terminal smoothness problem yet.
