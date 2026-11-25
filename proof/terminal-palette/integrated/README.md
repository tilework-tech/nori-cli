# Terminal palette startup after the stack rebase

Linux x86-64, optimized musl, 120×40 PTY, synthetic local sessions. Each cell is the median of five paired samples in milliseconds, baseline → candidate.

| Entry | Immediate, handled | Delayed, handled | Missing, handled | Missing, ready | Delayed, key after focus |
|---|---:|---:|---:|---:|---:|
| attach | 7.7 → 7.5 | 308.0 → 6.9 | 2,007.5 → 7.3 | 2,009.0 → 9.0 | 301.5 → 0.7 |
| attach-id | 8.0 → 7.6 | 308.4 → 7.6 | 2,008.6 → 8.2 | 2,026.9 → 28.3 | 302.2 → 1.1 |
| attach-remote | 7.6 → 7.5 | 308.1 → 7.9 | 2,007.9 → 7.2 | 2,009.8 → 9.0 | 301.6 → 1.0 |
| resume | 39.0 → 39.4 | 339.1 → 37.7 | 2,038.9 → 57.1 | 2,038.9 → 57.1 | 302.5 → 1.9 |
| plain | 40.2 → 41.3 | 365.2 → 63.8 | 2,044.2 → 56.5 | 2,068.8 → 83.9 | 302.1 → 1.3 |
| plain-pipe | 29.2 → 26.5 | 329.7 → 29.9 | 2,030.3 → 30.1 | 2,058.3 → 56.3 | 302.2 → 1.3 |
| cloud | 67.1 → 69.4 | 376.3 → 86.7 | 2,064.8 → 83.6 | 2,063.1 → 82.0 | 302.4 → 1.7 |
| host-ls | 7.2 → 6.8 | 307.7 → 6.7 | 2,007.7 → 6.9 | 2,007.7 → 6.9 | — |
| host-ls-pipe | 6.0 → 6.1 | — | — | — | — |

The delayed emulator schedules a response 150 ms after each request. Interactive readiness is an initialized model, populated list or restored tool, depending on route. The early exit probe types at the alternate-screen leave sequence. The `very-late` case delays replies by 450 ms, beyond the application's 200 ms maximum handoff window.

Startup gate 764.json passed 7/8. cold_host.p95 candidate 13.89 ms, ceiling 13.82 ms.

Startup gate 764-repeat.json passed 8/8. 

## Builds and terminal proof

Both binaries used Rust 1.90.0, the repository's locked release profile and the x86_64-unknown-linux-musl target. The base build used `e4255537757ba630f9ce083684d837a2099041c2`, which has the identical Rust source tree (`b2e7a5099403e13f7ceaffe3013f499f0581e970`) to the direct parent `3bce42d44119d56c0994b5d7b002841ba1764af5`. The parent adds Python fixtures and results. The repaired build used `a5daa5973c9f6ff680cfb9636167bb3d68450375`. Their respective executable SHA-256 hashes are `5e0e0d252b79d35cc08002712b5836795da7ae3b1bdea5010ad219df24054195` and `dbe2cf28e0dd068ea8db41565fa9a433f552450b4269e1c2911ebdd8ae6eefdb`.

A real, detached tmux terminal received δ at 35 ms. The [baseline pane](before.png) was still blank at 104.8 ms. The [repaired pane](after.png) showed two synthetic sessions and the draft at 104.7 ms. Both eventually displayed the rows and draft. The [before](before.txt) and [after](after.txt) text files preserve their screen contents.

The paired PTY benchmark used two seeded host sessions with synthetic Markdown and tool output. It rotated test cases and alternated binary order. [Raw paired samples](paired.json), [controls](controls.json), [baseline handoffs](base-handoffs.json), [repaired handoffs](handoffs.json) and [ranges by metric](summary.csv) cover first typed input, populated or restored content, subsequent keys, color refresh and terminal restoration.

If the terminal suppresses keyboard negotiation and device attributes it can still take roughly two seconds to start. If it suppresses the mandatory cursor-position report, setup fails and the repaired build restores canonical input. At handoff, terminal replies delayed beyond the remaining 200 ms window can reach the next owner.

With OSC 7501 program status enabled, a separate real PTY observed the two synthetic picker rows and idle state at 8.0 ms. On quit, Nori cleared program status and consumed delayed color and DSR responses before exposing the shell at 156.0 ms. The shell received the exact line typed when it first became visible, and the original terminal attributes were restored.

The [earlier full benchmark archive](https://github.com/tilework-tech/nori-cli/blob/eebcfae86c08ca45bfc56a69f6da2c4f1a55d23a/pr-773/evidence-a5daa597/MANIFEST.md) includes the shipped release and the pre-rebase comparison.

## Reproduce

From a local clone, build the two revisions separately, then run the PTY and original host startup checks from the candidate checkout. The harnesses use temporary synthetic homes. The cloud entry point uses a local synthetic adapter. Timings cover the PTY and local host on Linux.

```sh
git fetch origin pull/773/head
git worktree add --detach /tmp/nori-palette-base 3bce42d44119d56c0994b5d7b002841ba1764af5
git worktree add --detach /tmp/nori-palette-fixed a5daa5973c9f6ff680cfb9636167bb3d68450375
for checkout in /tmp/nori-palette-base /tmp/nori-palette-fixed; do
    (cd "$checkout/nori-rs" && cargo +1.90.0 build --locked --release --target x86_64-unknown-linux-musl -p nori-cli -j 4)
done
cd /tmp/nori-palette-fixed
base_bin=/tmp/nori-palette-base/nori-rs/target/x86_64-unknown-linux-musl/release/nori
fixed_bin=/tmp/nori-palette-fixed/nori-rs/target/x86_64-unknown-linux-musl/release/nori
python3 scripts/terminal-palette/measure.py --base "$base_bin" --binary "$fixed_bin" --fixtures scripts/attach_perf --samples 5 --check --output paired.json
python3 scripts/terminal-palette/measure.py --base "$base_bin" --binary "$fixed_bin" --fixtures scripts/attach_perf --paths attach plain --modes silent-all background-only device-silent --samples 3 --check --output controls.json
python3 scripts/terminal-palette/handoffs.py --binary "$fixed_bin" --fixtures scripts/attach_perf --samples 3 --check --output handoffs.json
python3 scripts/host_startup_benchmark.py --base "$base_bin" --head "$fixed_bin" --samples 20 --output 764.json
```
