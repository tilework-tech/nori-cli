# Recovering a failed adapter in alternate-screen attach

The failed row remains on its original host after the synthetic adapter exits. In the picker, Ctrl+R starts a replacement adapter for the same conversation ID and leaves the picker open. Enter subscribes to its journal. These are separate user actions. All 36 samples delivered a new prompt to the replacement adapter and displayed its acknowledgment.

| Build | Adapter history source | Sections | Ctrl+R to ready row, ms | Enter to first transcript content, ms | Enter to first latest reply, ms | Enter to new reply, ms |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| Release | Local transcript | 4 | 31.8 | 12.4 | 60.2 | 253 |
| Release | Native load | 4 | 35.2 | 12.8 | 51.7 | 221 |
| Stack | Local transcript | 4 | 32.4 | 13.1 | 61.8 | 269 |
| Stack | Native load | 4 | 34.1 | 14.3 | 55.3 | 289 |
| Fixed | Local transcript | 4 | 34.4 | 28.1 | 56.9 | 260 |
| Fixed | Native load | 4 | 34.1 | 5.8 | 70.1 | 271 |
| Release | Local transcript | 64 | 47.5 | 22.0 | 2,661 | 5,478 |
| Release | Native load | 64 | 71.4 | 16.7 | 2,647 | 5,494 |
| Stack | Local transcript | 64 | 51.4 | 25.0 | 2,606 | 5,348 |
| Stack | Native load | 64 | 71.9 | 26.9 | 2,632 | 5,474 |
| Fixed | Local transcript | 64 | 47.7 | 40.4 | 78.1 | 236 |
| Fixed | Native load | 64 | 70.2 | 34.7 | 73.2 | 282 |

Each cell is a median of three fresh hosts on Linux x86-64, using a 120×40 draining tmux pane and a loopback WebSocket recording proxy. The release is 0.30.0-next.6. The pinned stack binary and exact hashes are shared with the main replay comparison. Compilation also ran on the machine during this diagnostic. Across the six long samples per build, first latest reply ranged from 2.64–2.70 s on the release and 2.58–2.63 s on the stack. The new reply ranged from 5.44–5.60 s and 5.35–5.50 s respectively. On the fixed build the six long samples showed the latest historical reply in 61.3–87.3 ms and the new reply in 231.2–288.5 ms. All 12 fixed runs received contiguous sequence numbers and confirmed the applied cursor on reselection. Raw samples include per-process activation timestamps and transcript sizes.

The harness sends SIGTERM to the adapter process that logged the synthetic seed, waits for the failed row, and sends Ctrl+R through the terminal. A replacement without load support receives ACP initialize and session/new. A replacement advertising native load receives initialize and session/load and streams the same fixture. Four sections contain 43 ACP updates. Sixty-four sections contain 643. A new prompt after the saved reply distinguishes historical content from live work. That timing includes a deliberate 120 ms delay between typing and Enter to let paste detection settle.

The list's reply preview already contains the final answer, so the latest-reply milestone requires a match in the opened conversation body. Opening a recovered session receives the original journal followed by restoration. The first appearance of the latest reply can precede restoration finishing. The last-listed sequence can also precede the final replay watermark. The diagnostic checks contiguous wire sequence numbers and a replay-finished event, confirms a new agent reply on screen, and reads the applied cursor from the TUI's next subscription. The first run in each group also scrolls back through all numbered sections and checks their order.

A separate clean-host-restart control on each of the three builds kept the saved synthetic file and final tool marker on disk. Both the managed host list and the alternate-screen picker contained zero rows. The Ctrl+R benchmark therefore requires a failed row on the still-running host. Provider-owned inventory is covered by the startup and provider-resume benchmark.

```sh
python3 scripts/attach_perf/recovery.py /absolute/path/to/nori --turns 4 64 --samples 3 --output recovery.json
python3 scripts/attach_perf/recovery.py /absolute/path/to/nori --check-restart --output recovery-restart.json
```
