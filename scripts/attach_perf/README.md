# Measuring attached session replay

These Linux probes drive real Nori binaries against a real host and synthetic ACP subprocesses. Each sample creates a fresh home, trusted project, host and terminal. A tmux pane at 120×40 drains the controlling PTY continuously; the driver sends keys through tmux and reads the displayed cells. All transcripts, adapters and prompts are synthetic.

## Reproduce

Run from the repository root with Python 3.11+, tmux and two or more absolute binary paths. Build each revision in its own `CARGO_TARGET_DIR` and save the executable before switching:

```sh
(cd nori-rs && cargo +1.90.0 build --release --locked -p nori-cli -j 4)
```

Add `--target x86_64-unknown-linux-musl` to match the libc of the shipped Linux artifact. The libc changes absolute times, so compare builds of the same libc.

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
# Repeated long recovery with every sample audited (exits non-zero on any conclusive failure)
python3 scripts/attach_perf/recovery.py "$FIXED" --samples 8 --turns 128 160 --audit-all --output recovery-long.json
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
- **Recovery.** Ctrl+R on a failed row starts a replacement adapter (native load or local transcript fallback); Enter opens it; a new prompt must be answered. The audit pages back to the first event, stitches the overlapping screens into one scrollback, and fails unless every section appears exactly once, whole and in order, with no connection notice inside it. By default only the first sample of each group is audited; `--audit-all` audits every sample.
- **Startup.** Plain `nori` activation to first handled keystroke and visible initialized model; provider `/resume` list and restore; the unmodified #764 gates with initialize held open. Times are milliseconds from launch or Enter, ending when bytes reach the PTY reader.

Each recovery sample ends in one labelled outcome, and the run ends with a tally:

| Outcome | Meaning | Conclusive |
|---|---|---|
| `pass` | Restored, audited if selected, and the new prompt was answered | yes |
| `notice_in_history` | A connection notice split the restored history | yes |
| `history_mismatch` | A section is missing, repeated, damaged or out of order | yes |
| `prompt_lost` | A reconnect dropped the new prompt ("Connection restored. A previous request…") | yes |
| `no_reply` | The prompt left the composer and no answer arrived | yes |
| `replay_unavailable` | The history exceeded what the host journal retains | yes |
| `unsent_draft` | The prompt was still in the composer when the wait ended | no |
| `error` | Any other harness failure | no |

An inconclusive outcome means the harness cannot attribute the miss to the build. Rerun the sample and keep the screen in the result.

The 64-section default rarely restores fast enough to expose a dropped subscription. Use the repeated 128–160 section command above when checking recovery after a host change.

Workloads: 4 sections (43 ACP updates, 741 text bytes) and 64 sections (643 updates, 11,042 bytes, ~258 KB transcript). Each section has user text, chunked Markdown, a fenced Rust block and a completed read tool. Opening results cover 4–64 sections; recovery also ran at 128–220. The host retains at most 4,096 envelopes across all of its sessions, so a much longer history opens as "Replay unavailable" with an empty conversation on every build measured. Recovery holds both the failed and the restored history in that journal, so it reaches the limit at about half the size: every sample up to 200 sections (2,003 updates) passed, sizes from 216 sections could show "Replay unavailable", and every size from 225 sections did.

## Results

Linux x86-64 (Modal, 4 vCPU, kernel 7.3.4), tmux 3.3a, loopback, warm pages. 64 sections, one listed row, median of five fresh hosts in milliseconds. The shipped column is the released musl artifact (binary SHA-256 `671118f0…`). The other two are glibc builds of the stack as it stands: the #772 head `d8c061da` that #769 builds on, and #769 at `085dac66`. Compare the two glibc columns; the shipped column shows the released starting point, and the libc and the machine both move its absolute times.

| Milestone | Shipped next.6 (musl) | #772 `d8c061da` | #769 `085dac66` |
|---|---:|---:|---:|
| Enter to first history content | 10.0 | 6.0 | 10.4 |
| Enter to latest restored reply | 1,249 | 741 | 51.8 |
| Enter to final saved tool | 1,252 | 742 | 51.8 |
| PageUp to body change | 5.3 | 5.1 | 2.2 |
| Draws during replay | 661 | 662 | 16 |
| Failed-row recovery (local transcript), Enter to latest reply, median of 3 | 1,235 | 741 | 26.3 |

Both glibc builds applied all 339 ordered events in the 32-section wire check. Opening a 150-section history in the independent harness on `nori/host-tui-acceptance-769-0e673ef4` (3,225 updates) took 48–63 ms on #769 against 16.7 s on #772, with no disconnect on #769 (n=3 each).

Recovery at 128 and 160 sections, eight samples per size and mode, every sample audited: #769 at `085dac66` passed 32 of 32. The previous #769 head `e2e98ba3` passed 31 of 32, failing once with `notice_in_history` at 160 sections. No sample on either build ended as `unsent_draft`.

An earlier run on musl builds of the pre-rebase heads (stack `96e84746`, fix `c3e3d4fa`) measured 2,562 and 56.8 ms from Enter to the latest restored reply, and all eight #764 startup gates passed for that fix. The startup gates were not rerun on the current heads.

Raw results, binary hashes and the stress comparison for these tables are in [`pr-769/residual-085dac66`](https://github.com/tilework-tech/nori-cli/blob/5d153957413b781da4589ddb9c53c4985225848e/pr-769/residual-085dac66/README.md) on `nori/pr-proofs`. The links below hold the earlier musl run.

[`example/replay-fixed-64-1.json`](example/replay-fixed-64-1.json) is one raw replay result from the fixed build. The [complete raw results, terminal captures, CPU profile, timing samples and build metadata](https://github.com/tilework-tech/nori-cli/blob/eebcfae86c08ca45bfc56a69f6da2c4f1a55d23a/pr-770/evidence-3bce42d4/MANIFEST.md) are preserved on the `nori/pr-proofs` branch, including the full [replay](https://github.com/tilework-tech/nori-cli/blob/eebcfae86c08ca45bfc56a69f6da2c4f1a55d23a/pr-770/evidence-3bce42d4/README.md), [startup](https://github.com/tilework-tech/nori-cli/blob/eebcfae86c08ca45bfc56a69f6da2c4f1a55d23a/pr-770/evidence-3bce42d4/startup.md) and [recovery](https://github.com/tilework-tech/nori-cli/blob/eebcfae86c08ca45bfc56a69f6da2c4f1a55d23a/pr-770/evidence-3bce42d4/evidence/recovery.md) write-ups.
