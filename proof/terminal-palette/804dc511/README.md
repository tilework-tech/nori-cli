# Terminal palette startup after the stack rebase

Linux x86-64, optimized musl, 120×40 PTY, synthetic local sessions. Each cell is the median of five paired samples in milliseconds, baseline → candidate.

| Entry | Immediate, handled | Delayed, handled | Missing, handled | Missing, ready | Delayed, key after focus |
|---|---:|---:|---:|---:|---:|
| attach | 8.4 → 8.0 | 308.8 → 7.6 | 2,008.5 → 7.8 | 2,009.9 → 9.6 | 301.4 → 0.7 |
| attach-id | 8.1 → 7.8 | 308.5 → 7.6 | 2,007.9 → 7.4 | 2,026.4 → 26.6 | 302.2 → 1.2 |
| attach-remote | 7.9 → 7.5 | 308.7 → 7.7 | 2,008.1 → 7.6 | 2,009.8 → 9.4 | 301.6 → 1.0 |
| resume | 62.0 → 36.7 | 344.4 → 62.4 | 2,052.1 → 66.3 | 2,052.1 → 66.2 | 302.5 → 2.0 |
| plain | 66.6 → 41.6 | 365.9 → 67.9 | 2,067.1 → 55.4 | 2,093.7 → 82.4 | 301.8 → 1.3 |
| plain-pipe | 30.2 → 27.4 | 330.7 → 29.0 | 2,029.2 → 27.9 | 2,056.0 → 54.9 | 302.1 → 1.3 |
| cloud | 65.2 → 64.3 | 372.9 → 88.4 | 2,063.9 → 65.6 | 2,062.3 → 64.0 | 302.3 → 1.6 |
| host-ls | 7.5 → 7.4 | 308.2 → 7.4 | 2,007.7 → 7.4 | 2,007.7 → 7.4 | — |
| host-ls-pipe | 6.6 → 6.6 | — | — | — | — |

The delayed emulator schedules a response 150 ms after each request. Interactive readiness is an initialized model, populated list or restored tool, depending on route. The early exit probe types at the alternate-screen leave sequence. The `very-late` case delays replies by 450 ms, beyond the application's 200 ms maximum handoff window.

Startup gate 764.json passed 8/8. 

Source commits: parent `588423e7a7111f0bd669cbea93ddf82ef5731922`, candidate `804dc5119f7c4b6e255cd54e5df6eea2489df2a7`.

Parent binary SHA-256 `b34747e59b47b3181518d65eddfcc37885f81acf611a6b6e11e86768f5bbc888`. Candidate binary SHA-256 `e8a3986a57881f1cea65b6386cb0aba50db0bad85b9e74fe482a369b3d1bbbd9`.

The fixed attach first handled input ranged from 7.0–7.9 ms across the five missing-color samples. With only the background response, its median first key was 7.6 ms and the expected stripe appeared at 157.5 ms. Focus changed the rendered dark stripe. A fully silent terminal fails at its mandatory cursor-position query before colors, after 2026.9 ms; all terminal modes were restored. Ignoring keyboard capability/device attributes while answering cursor and color queries costs about 2,008 ms before the first handled key.

All 150 ms delayed handoffs on attach, native resume and full-screen quit delivered exactly the command typed at alternate-screen leave. The median rapid quit-to-shell time was 147.6 ms with 150 ms replies, and 198.3 ms when the emulator also ignored the status barrier. In all intentionally late 450 ms samples the new owner received two stray OSC replies and one status reply, alongside its typed command. Six independent editor/job-control transitions on the candidate restored the TTY, preserved the next owner's line and accepted input after the application regained control.

## Restored history and interleaved live output

- base, transcript. Audited 64 complete, ordered sections over 973 stitched lines. The replacement session answered a real follow-up and the reselected UI's cursor was 1316.
- base, native. Audited 64 complete, ordered sections over 972 stitched lines. The replacement session answered a real follow-up and the reselected UI's cursor was 1315.
- candidate, transcript. Audited 64 complete, ordered sections over 973 stitched lines. The replacement session answered a real follow-up and the reselected UI's cursor was 1316.
- candidate, native. Audited 64 complete, ordered sections over 972 stitched lines. The replacement session answered a real follow-up and the reselected UI's cursor was 1315.
- base, live catch-up. 339 events, 0 gaps, saved watermark 334, and cursor 339 through 339 delivered on reselection. Live plus ready became visible 59.7 ms after selection. This independent 32-section diagnostic is one run per executable.
- candidate, live catch-up. 339 events, 0 gaps, saved watermark 334, and cursor 339 through 339 delivered on reselection. Live plus ready became visible 58.1 ms after selection. This independent 32-section diagnostic is one run per executable.

The failed-row recovery numbers use one fresh 64-section history and adapter per mode/build. Millisecond timings on synthetic loopback have scheduler variability. Neither native platform display scanout nor a real remote provider is covered.

## Builds and terminal proof

Both binaries used Rust 1.90.0, the repository's locked release profile and the x86_64-unknown-linux-musl target. The parent was `588423e7a7111f0bd669cbea93ddf82ef5731922` and the repaired build was `804dc5119f7c4b6e255cd54e5df6eea2489df2a7`. Their respective executable SHA-256 hashes are `b34747e59b47b3181518d65eddfcc37885f81acf611a6b6e11e86768f5bbc888` and `e8a3986a57881f1cea65b6386cb0aba50db0bad85b9e74fe482a369b3d1bbbd9`.

A real, detached tmux terminal received δ at 35 ms. The [baseline pane](before.png) was still blank at 104.7 ms. The [repaired pane](after.png) showed two synthetic sessions and the draft at 104.5 ms. Both eventually displayed the rows and draft. The [before](before.txt) and [after](after.txt) text files preserve their screen contents.

The paired PTY benchmark used two seeded host sessions with synthetic Markdown and tool output. It rotated test cases and alternated binary order. [Raw paired samples](paired.json), [controls](controls.json), [baseline handoffs](base-handoffs.json), [repaired handoffs](handoffs.json) and [ranges by metric](summary.csv) cover first typed input, populated or restored content, subsequent keys, color refresh and terminal restoration.

If the terminal suppresses keyboard negotiation and device attributes it can still take roughly two seconds to start. If it suppresses the mandatory cursor-position report, setup fails and the repaired build restores canonical input. At handoff, terminal replies delayed beyond the remaining 200 ms window can reach the next owner.

The [64-section recovery traces on the parent](base-recovery.json) and [repair](candidate-recovery.json) record transcript fallback and native adapter recovery. The [32-section transport check](wire.json) follows a live event during replay and the applied cursor on reselection.

The [original pinned measurements and captures](https://github.com/tilework-tech/nori-cli/blob/eebcfae86c08ca45bfc56a69f6da2c4f1a55d23a/pr-773/evidence-a5daa597/MANIFEST.md) and [first integrated run](https://github.com/tilework-tech/nori-cli/blob/fc08d5c9af1a80280e0fa8ef7600df31324c4d14/proof/terminal-palette/integrated/README.md) remain available. The code PR retains the probes and one 30-row sample.

## Reproduce

From a local clone, build the two revisions separately, then run the PTY and original host startup checks from the candidate checkout. The harnesses use temporary synthetic homes. The cloud entry point uses a local synthetic adapter. Timings cover the PTY and local host on Linux.

```sh
git fetch origin pull/773/head
git worktree add --detach /tmp/nori-palette-base 588423e7a7111f0bd669cbea93ddf82ef5731922
git worktree add --detach /tmp/nori-palette-fixed 804dc5119f7c4b6e255cd54e5df6eea2489df2a7
for checkout in /tmp/nori-palette-base /tmp/nori-palette-fixed; do
    (cd "$checkout/nori-rs" && cargo +1.90.0 build --locked --release --target x86_64-unknown-linux-musl -p nori-cli -j 4)
done
cd /tmp/nori-palette-fixed
base_bin=/tmp/nori-palette-base/nori-rs/target/x86_64-unknown-linux-musl/release/nori
fixed_bin=/tmp/nori-palette-fixed/nori-rs/target/x86_64-unknown-linux-musl/release/nori
python3 scripts/terminal-palette/measure.py --base "$base_bin" --binary "$fixed_bin" --fixtures scripts/attach_perf --samples 5 --check --output paired.json
python3 scripts/terminal-palette/measure.py --base "$base_bin" --binary "$fixed_bin" --fixtures scripts/attach_perf --paths attach plain --modes silent-all background-only device-silent --samples 3 --check --output controls.json
python3 scripts/terminal-palette/handoffs.py --binary "$fixed_bin" --fixtures scripts/attach_perf --samples 3 --check --output handoffs.json
python3 scripts/attach_perf/recovery.py "$fixed_bin" --samples 1 --turns 64 --output recovery.json
python3 scripts/attach_perf/transport.py --build parent "$base_bin" --build candidate "$fixed_bin" --turns 32 --output wire.json
python3 scripts/host_startup_benchmark.py --base "$base_bin" --head "$fixed_bin" --samples 20 --output 764.json
```
