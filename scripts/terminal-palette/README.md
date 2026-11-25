# Measuring terminal palette startup

These Linux probes start optimized binaries on a 120×40 controlling PTY that drains output continuously and answers cursor and device queries. In truecolor mode it answers OSC 10 and 11 immediately, 150 ms after each query, or never. Two ready synthetic sessions are seeded on a local host from the `scripts/attach_perf` fixtures. Plain `nori` runs a synthetic ACP process; `nori cloud` runs it behind a local `NORI_HANDROLL_BIN`.

The first keystroke is sent when the first palette query arrives or the first frame appears. Echo is off, so a typed character counts only once the application renders it. With `--check`, each run must also render a key after focus, restore terminal attributes on exit, balance alternate-screen transitions and leave shell typeahead to the next owner. Ready means a populated picker, the last saved tool for direct attach, or an initialized model for plain startup. Timings end when bytes reach the PTY reader; every `ms_` column starts at process launch.

## Reproduce

Use Python 3.11+ from the repository root. Build the base and candidate separately and keep both binaries:

```sh
(cd nori-rs && cargo +1.90.0 build --release --locked --target x86_64-unknown-linux-musl -p nori-cli -j 4)

# Entry points × immediate, delayed and missing palette replies
python3 scripts/terminal-palette/measure.py --fixtures scripts/attach_perf \
    --base "$BASE" --binary "$FIXED" --samples 5 --check --output palette-comparison.json
# Background-only replies, every query unanswered, device attributes withheld
python3 scripts/terminal-palette/measure.py --fixtures scripts/attach_perf \
    --base "$BASE" --binary "$FIXED" --paths attach plain --modes background-only silent-all device-silent \
    --samples 3 --check --output palette-controls.json
# Ctrl+G editor and Ctrl+Z/fg job handoff
python3 scripts/terminal-palette/handoffs.py --fixtures scripts/attach_perf \
    --binary "$FIXED" --samples 3 --check --output palette-handoffs.json
# PR #764 eight-gate startup benchmark (responding terminal, initialize held open)
python3 scripts/host_startup_benchmark.py --base "$BASE" --head "$FIXED" --samples 20 --output palette-764.json
```

## Results

Linux x86-64 (Modal, kernel 7.3.4), warm musl builds, five rounds alternating base `80992049` and the repair. Median launch to first handled input, base → repair, milliseconds:

| Entry point | Immediate | 150 ms per reply | No palette reply |
|---|---:|---:|---:|
| Attach picker | 8.2 → 7.1 | 308.3 → 7.1 | 2,007.5 → 7.1 |
| Saved-session picker | 37.0 → 35.8 | 337.2 → 36.8 | 2,037.1 → 37.5 |
| Fresh inline composer | 39.2 → 37.5 | 340.5 → 61.2 | 2,057.9 → 49.3 |
| Cloud picker, synthetic adapter | 59.4 → 84.2 | 383.4 → 61.0 | 2,069.5 → 84.7 |
| Host list on a tty | 7.1 → 7.1 | 307.0 → 6.9 | 2,007.0 → 7.0 |

The shipped 0.30.0-next.6 binary measured 7.1, 307.6 and 2,007.8 ms on the attach picker. Every delayed-reply handoff returned the full shell command, and editor and suspended-job owners received no terminal reply bytes. An unanswered device-attributes query still costs about two seconds independently.

[`example/attach-samples.csv`](example/attach-samples.csv) holds the 30 raw attach-picker rows behind the first table row. The [complete raw samples, handoff results, #764 gate runs, before/after terminal captures and build metadata](https://github.com/tilework-tech/nori-cli/blob/eebcfae86c08ca45bfc56a69f6da2c4f1a55d23a/pr-773/evidence-a5daa597/MANIFEST.md) are preserved on the `nori/pr-proofs` branch, with the [full write-up](https://github.com/tilework-tech/nori-cli/blob/eebcfae86c08ca45bfc56a69f6da2c4f1a55d23a/pr-773/evidence-a5daa597/README.md).
