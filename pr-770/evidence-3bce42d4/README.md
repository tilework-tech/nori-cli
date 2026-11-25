# Measuring attached session replay

Run from the repository root with two or more absolute executable paths and tmux. The benchmark creates a new local home, trusted project, host, synthetic ACP subprocesses and terminal for each sample. The tmux server continuously drains a controlling PTY and interprets the terminal output at 120 columns by 40 rows. The driver sends keys through tmux and observes the displayed cells. Each additional listed session is a real host session with a short reply.

## Reproduce

The Linux release artifact is `nori-0.30.0-next.6-linux-x86_64.tar.gz` from tag `nori-v0.30.0-next.6`, peeled commit `b999c7131e5849356d4780193c34f50eddb2609b`. The archive SHA-256 is `5d0c8907decf4e6075203014eb1386c02d9655059a77fe957730b5a86c15961e`. Extract it and make the member `nori/nori` executable. Use the same optimized target for the stack (`96e847467329e5b19badc75f6126e04251630038`) and the fix (`c3e3d4fa0ad499e99730fe84a486c82ffaa7f9b7`). In each respective checkout on x86-64 Linux with Rust 1.90.0, musl-tools, clang and mold, run

```sh
(cd nori-rs && cargo +1.90.0 build --release --locked --target x86_64-unknown-linux-musl -p nori-cli -j 4)
```

Save each executable before switching revisions. Point RELEASE, STACK and FIXED at those binaries. Set NORI_BENCH_TMUX to the isolated tmux wrapper when running inside a Nori session. Otherwise the driver creates a private tmux socket in its temporary directory.

```sh
python3 scripts/attach_perf/replay.py "$RELEASE" --turns 64 --sessions 1 --samples 5 --live --stream --output release.json
python3 scripts/attach_perf/replay.py "$STACK" --turns 64 --sessions 1 --samples 5 --live --stream --output stack.json
python3 scripts/attach_perf/replay.py "$FIXED" --turns 64 --sessions 1 --samples 5 --live --stream --check-batching --output fixed.json
python3 scripts/attach_perf/replay.py "$FIXED" --turns 32 --samples 1 --live --audit --screen fixed-screen.txt
python3 scripts/attach_perf/transport.py --build release "$RELEASE" --build stack "$STACK" --build fixed "$FIXED" --turns 32 --output wire.json
python3 scripts/attach_perf/recovery.py "$FIXED" --samples 3 --turns 4 64 --output recovery.json
python3 -m unittest discover -s scripts/attach_perf
```

For the full direct-terminal matrix use 4 and 64 sections with 1 and 24 listed sessions. Release and stack samples rotated in order within each configuration over five rounds. Repeat the same configurations on the final build. To examine CPU attribution, build a symbol-retaining optimized stack with `CARGO_PROFILE_RELEASE_STRIP=none`, then run

```sh
python3 scripts/attach_perf/replay.py "$SYMBOLIZED_STACK" --turns 64 --samples 1 --live --profile stack.perf
perf report --stdio --children --sort symbol -g none -i stack.perf
```

## What each probe measures

- **Open a host row.** Start `nori attach` against the already running host. Wait for a populated selectable list in the alternate screen. With multiple rows, filter to the target first. Start selection timing immediately before Enter. Record the first history content, the latest restored agent reply, and a distinct completed read tool at the end of the saved history. Type § after the first content and check the composer. The optional live injection starts an independent agent turn during this sequence.
- **Interact.** After catch-up, type another character, scroll up ten lines, and observe the changed body. The optional live stream emits 24 chunks at 20 ms intervals. Type while that stream is in progress. The timers include tmux command dispatch and screen polling. Two close milestones can fall in the same observation. A null scroll time means that the history already fits.
- **Verify completion.** The final visible tool precedes trailing host status events. The separate wire probe records the initial saved watermark, injects a live update, and checks the UI's own subscription cursor on reselection. It rejects missing sequences, out-of-order markers, or a cursor short of any event already delivered. The reselection time bounds completion from above. The scroll audit also searches for each numbered section back to the first marker. Screen digests use the settled transcript body.
- **Recover a failed session.** Ctrl+R on a failed host row initializes the replacement adapter and leaves the picker open. Enter then opens the recovered history. Exercise both adapter-native load and local transcript fallback. Send a new prompt and observe its reply to verify usability. This diagnostic also checks what the picker offers after stopping and restarting the host.
- **Start a fresh CLI or open the provider's saved picker.** The original PR #764 startup probe, agent-ready probe, plain-terminal live stream and provider `/resume` measurements have their own commands and milestone definitions in [startup.md](startup.md). The earlier S44 verifier covered plain-TUI screen and resume correctness on the acceptance branch at `4ffa85969b04fd7e20268bee960a675e1d078c00`.

The short workload has 4 Markdown sections, 43 ACP updates and 741 bytes of message text. The long workload has 64 sections, 643 ACP updates and 11,042 bytes. Each section contains user text, a chunked Markdown answer, a fenced Rust example and a structured read tool with a completion update. The long persisted transcript is about 258 KB. A real provider or network can add independent delays.

The measurements used a Modal x86-64 Linux session, kernel 7.3.4, tmux 3.3a, local loopback, warm binary pages, and fresh homes. Release and local builds use musl, release optimization, fat LTO and one codegen unit. Host setup and synthetic history generation happen before the direct attach measurements. Normal runs configure tmux's default colors so it answers the optional OSC queries. Repeat with `--unanswered-colors` to measure a terminal withholding its palette.

## Findings

**64 sections, one listed conversation.** Each cell is the median of five fresh hosts, followed by the observed minimum and maximum. Times are milliseconds.

| Milestone | Shipped next.6 | Pinned stack | Fixed stack |
|---|---:|---:|---:|
| Populated list on launch | 12.8 (9.0–13.4) | 9.8 (8.5–13.2) | 9.4 (8.3–12.7) |
| Enter to first history content | 14.6 (12.3–19.4) | 14.3 (11.6–17.7) | 34.6 (7.3–40.2) |
| Enter to handled draft | 32.9 (30.0–35.9) | 36.0 (33.0–38.3) | 46.0 (32.5–52.0) |
| Enter to latest restored reply | 2,581 (2,564–2,602) | 2,562 (2,555–2,590) | 56.8 (50.5–61.3) |
| Enter to final saved tool | 2,591 (2,573–2,607) | 2,568 (2,565–2,600) | 56.8 (50.5–61.3) |
| Enter to live reply | 2,644 (2,627–2,661) | 2,623 (2,618–2,652) | 61.3 (56.8–91.1) |
| Settled character | 15.4 (15.1–15.7) | 15.4 (11.3–16.3) | 5.7 (5.5–5.9) |
| PageUp to body change | 14.9 (10.9–15.6) | 15.3 (14.9–16.4) | 5.9 (5.6–7.5) |
| Character during paced stream | 11.4 (10.8–15.6) | 15.5 (12.2–16.0) | 5.8 (5.4–6.0) |
| Draws during replay | 662 (662–662) | 663 (663–663) | 17.0 (15.0–17.0) |
| Terminal bytes during replay | 1,036,630 (1,036,629–1,036,634) | 1,037,478 (1,037,477–1,037,498) | 29,867 (26,962–30,998) |

**List and history sizes.** Times use the same five-sample convention.

| Sections | Listed rows | Milestone | Shipped next.6 | Pinned stack | Fixed stack |
|---:|---:|---|---:|---:|---:|
| 4 | 1 | Populated list on launch | 13.2 (10.7–15.5) | 10.6 (9.4–14.7) | 9.8 (8.8–13.5) |
| 4 | 1 | Enter to first history content | 10.4 (7.8–53.5) | 50.0 (10.5–54.8) | 23.3 (5.9–25.8) |
| 4 | 1 | Enter to latest restored reply | 55.4 (52.4–90.4) | 91.0 (50.6–95.4) | 51.1 (23.3–72.1) |
| 4 | 24 | Populated list on launch | 13.9 (13.0–15.2) | 13.7 (13.4–15.9) | 14.7 (13.8–16.9) |
| 4 | 24 | Enter to first history content | 49.3 (11.6–56.3) | 13.3 (10.4–14.5) | 29.8 (24.8–70.2) |
| 4 | 24 | Enter to latest restored reply | 88.0 (48.8–93.3) | 64.1 (50.7–89.7) | 29.8 (24.8–70.2) |
| 64 | 24 | Populated list on launch | 13.8 (12.8–15.5) | 14.7 (13.8–15.2) | 14.0 (12.2–15.5) |
| 64 | 24 | Enter to first history content | 17.9 (13.2–18.9) | 13.0 (12.8–19.7) | 32.6 (7.7–35.4) |
| 64 | 24 | Enter to latest restored reply | 2,596 (2,582–2,621) | 2,603 (2,571–2,616) | 50.8 (45.9–64.3) |

The 64-section profile sampled the real attach process at 199 Hz. It collected 430 user CPU samples with zero lost. Inclusive call chains contained Markdown formatting in 37.0% of samples, code highlighting in 32.6%, and conversation rendering in 38.6%. Call-chain truncation and inlining limit attribution. These percentages overlap. The symbol-retaining profile binary was relinked from the same optimized object as the timed stack. Both .text sections had SHA-256 `25978bd11612af8c1b957971a2d7087cffc471dc5a41a1cb64c827953a07dede`. See [the profile](evidence/stack-cpu.txt).

Detached tmux with no default color response spent about two extra seconds before showing a list. In a query trace, the shipped binary issued OSC 10 at 9 ms, entered the alternate screen at 2,009 ms, and showed the list at 2,011 ms. Giving the same terminal explicit colors let it answer both color queries and show the list at 10 ms. The synchronous foreground-color probe in the pinned crossterm fork polls for 2,000 ms. A separate three-run control reproduced the delay on both versions. This startup wait precedes the Enter used to time replay.

The local adapter and warmed binary pages bound the scope of the startup measurements. A hosted provider, its authentication, network latency, and a cold machine can add latency. Tail rendering still formats an entire single visible message or tool payload. Reading far back visits the newer parts to locate the scroll offset. The test records terminal observation and applied cursors. A cursor sampled on reselection establishes completeness by that observation time.

In the 64-section, single-row case the latest reply reached the terminal about 45× sooner after the fix. The first identifiable history marker appeared later in this batch of samples (median 34.6 ms versus 14.3 ms), and the draft typed after that marker became visible at 46.0 ms versus 36.0 ms from selection. Timings for that first marker varied by tens of milliseconds. The entire retained conversation and live update arrived in reading order. In the independent 32-section wire check the fixed client's next subscribe request reported cursor 339 after all 339 envelopes, including trailing status. The saved watermark crossed the relay at 8.9 ms, and the live reply and ready state were visible at 36.8 ms. The stack needed 775.7 ms for the same visible endpoint. See [the diagnostic events](evidence/transport.json) and [the matching settled screen](evidence/stack-screen.txt).

Failed-adapter recovery is the highest impact case tested. The original history can appear once from retained envelopes and then be restored again by the replacement adapter. A reply appearing the first time leaves work outstanding. The [recovery results](evidence/recovery.md) include the ready row, first restored reply and successful new prompt. The [startup results](startup.md) cover fresh input, initialization, provider inventory and the separate host-startup gates.
