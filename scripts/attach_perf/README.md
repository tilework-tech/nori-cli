# Measuring attached session replay

These Linux probes drive real Nori binaries against a real host and synthetic ACP subprocesses. Each sample creates a fresh home, trusted project, host and terminal. A tmux pane at 120×40 drains the controlling PTY continuously; the driver sends keys through tmux and reads the displayed cells. All transcripts, adapters and prompts are synthetic.

## Reproduce

Run from the repository root with Python 3.11+, tmux and two or more absolute binary paths. Build each revision in its own `CARGO_TARGET_DIR` and save the executable before switching:

```sh
(cd nori-rs && cargo +1.90.0 build --release --locked --target x86_64-unknown-linux-musl -p nori-cli -j 4)
```

The shipped baseline is `nori-0.30.0-next.6-linux-x86_64.tar.gz` from tag `nori-v0.30.0-next.6` (SHA-256 `5d0c8907decf4e6075203014eb1386c02d9655059a77fe957730b5a86c15961e`). Point RELEASE, STACK and FIXED at the binaries. Inside a Nori session set NORI_BENCH_TMUX to the isolated tmux wrapper; otherwise the driver uses a private tmux socket.

```sh
# Direct attach replay (repeat with --turns 4 and --sessions 24 for the full matrix)
python3 scripts/attach_perf/replay.py "$RELEASE" --turns 64 --sessions 1 --samples 5 --live --stream --output release.json
python3 scripts/attach_perf/replay.py "$FIXED" --turns 64 --sessions 1 --samples 5 --live --stream --check-batching --output fixed.json
python3 scripts/attach_perf/replay.py "$FIXED" --turns 32 --samples 1 --live --audit --screen fixed-screen.txt
# Terminal withholding its palette
python3 scripts/attach_perf/replay.py "$STACK" --turns 1 --samples 3 --unanswered-colors --output color-timeout.json
# CPU attribution (symbol-retaining build: CARGO_PROFILE_RELEASE_STRIP=none)
python3 scripts/attach_perf/replay.py "$SYMBOLIZED_STACK" --turns 64 --samples 1 --live --profile stack.perf
# Wire ordering and applied cursor
python3 scripts/attach_perf/transport.py --build release "$RELEASE" --build fixed "$FIXED" --turns 32 --output wire.json
# Failed-adapter recovery and clean-restart control
python3 scripts/attach_perf/recovery.py "$FIXED" --samples 3 --turns 4 64 --output recovery.json
python3 scripts/attach_perf/recovery.py "$FIXED" --check-restart --output recovery-restart.json
# Fresh CLI startup, provider /resume, host trace and the PR #764 gates
python3 scripts/host_startup_benchmark.py --base "$RELEASE" --head "$STACK" --samples 20 --output startup-764.json
python3 scripts/attach_perf/startup.py --benchmark scripts/host_startup_benchmark.py --binary "$RELEASE" --binary "$STACK" --samples 10 --output startup-ready.json
python3 scripts/attach_perf/startup.py --benchmark scripts/host_startup_benchmark.py --binary "$RELEASE" --binary "$STACK" --samples 5 --live-chunks 40 --output startup-live.json
python3 scripts/attach_perf/startup.py --benchmark scripts/host_startup_benchmark.py --binary "$RELEASE" --binary "$STACK" --samples 5 --saved-rows 128 --turns 128 --output provider-long.json
python3 scripts/attach_perf/startup.py --benchmark scripts/host_startup_benchmark.py --binary "$STACK" --samples 3 --host-trace --output startup-host-trace.json
# Diagnostic regression tests
python3 -m unittest discover -s scripts/attach_perf
```

## Milestones

- **Replay.** Timing starts immediately before Enter on a populated host row. It records the first history content, the latest restored reply, a distinct final read tool, a draft typed after the first content, PageUp movement, and a character typed during an optional 24-chunk live stream. The audit scrolls back through every numbered section.
- **Wire.** The recording proxy rejects missing or out-of-order sequences and reads the TUI's applied cursor from its next subscription on reselection.
- **Recovery.** Ctrl+R on a failed row starts a replacement adapter (native load or local transcript fallback); Enter opens it; a new prompt must be answered.
- **Startup.** Plain `nori` activation to first handled keystroke and visible initialized model; provider `/resume` list and restore; the unmodified #764 gates with initialize held open. Times are milliseconds from launch or Enter, ending when bytes reach the PTY reader.

Workloads: 4 sections (43 ACP updates, 741 text bytes) and 64 sections (643 updates, 11,042 bytes, ~258 KB transcript). Each section has user text, chunked Markdown, a fenced Rust block and a completed read tool.

## Results

Linux x86-64 (Modal, kernel 7.3.4), tmux 3.3a, loopback, warm pages. 64 sections, one listed row, median of five fresh hosts in milliseconds:

| Milestone | Shipped next.6 | Stack `96e84746` | Fix #769 `c3e3d4fa` |
|---|---:|---:|---:|
| Enter to first history content | 14.6 | 14.3 | 34.6 |
| Enter to latest restored reply | 2,581 | 2,562 | 56.8 |
| Enter to final saved tool | 2,591 | 2,568 | 56.8 |
| PageUp to body change | 14.9 | 15.3 | 5.9 |
| Draws during replay | 662 | 663 | 17 |
| Failed-row recovery (local transcript), Enter to latest reply, median of 3 | 2,661 | 2,606 | 78.1 |

The fixed client applied all 339 ordered events in the 32-section wire check. All eight #764 startup gates passed for the fix against the stack. A terminal that withholds OSC 10/11 adds about two seconds before the picker on these builds.

[`example/replay-fixed-64-1.json`](example/replay-fixed-64-1.json) is one raw replay result from the fixed build. The [complete raw results, terminal captures, CPU profile, timing samples and build metadata](https://github.com/tilework-tech/nori-cli/blob/eebcfae86c08ca45bfc56a69f6da2c4f1a55d23a/pr-770/evidence-3bce42d4/MANIFEST.md) are preserved on the `nori/pr-proofs` branch, including the full [replay](https://github.com/tilework-tech/nori-cli/blob/eebcfae86c08ca45bfc56a69f6da2c4f1a55d23a/pr-770/evidence-3bce42d4/README.md), [startup](https://github.com/tilework-tech/nori-cli/blob/eebcfae86c08ca45bfc56a69f6da2c4f1a55d23a/pr-770/evidence-3bce42d4/startup.md) and [recovery](https://github.com/tilework-tech/nori-cli/blob/eebcfae86c08ca45bfc56a69f6da2c4f1a55d23a/pr-770/evidence-3bce42d4/evidence/recovery.md) write-ups.
