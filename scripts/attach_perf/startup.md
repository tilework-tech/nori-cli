# Startup and provider-saved sessions

Run these probes from the repository root on Linux with Python 3. Use two optimized binaries on the same machine, with the synthetic adapter on a local disk. These commands fetch the original benchmark from its pinned PR #764 commit in the local git object database.

```sh
git show 36af1c0fd4b39008c20d28c60742f68717b37fa9:scripts/host_startup_benchmark.py > /tmp/host_startup_benchmark.py
python3 /tmp/host_startup_benchmark.py --base "$RELEASE" --head "$STACK" --samples 20 --output startup-764.json

python3 scripts/attach_perf/startup.py --benchmark /tmp/host_startup_benchmark.py \
  --binary "$RELEASE" --binary "$STACK" --samples 10 --output startup-ready.json
python3 scripts/attach_perf/startup.py --benchmark /tmp/host_startup_benchmark.py \
  --binary "$RELEASE" --binary "$STACK" --samples 5 --saved-rows 4 --turns 4 --output provider-short.json
python3 scripts/attach_perf/startup.py --benchmark /tmp/host_startup_benchmark.py \
  --binary "$RELEASE" --binary "$STACK" --samples 5 --saved-rows 128 --turns 128 --output provider-long.json
python3 scripts/attach_perf/startup.py --benchmark /tmp/host_startup_benchmark.py \
  --binary "$STACK" --samples 3 --host-trace --output startup-host-trace.json
```

The #764 benchmark holds the agent's initialize response open. It times the plain composer's first frame and its first displayed keystroke, standalone host launch until the command returns, and a warm attach until an empty session list appears. It runs two release controls and one stack sample in rotating order.

The activation probe starts plain `nori --skip-trust-directory` in a fresh, trusted home and sends § on receipt of the composer frame. Terminal echo is off. Its first handled input is the first PTY output containing the typed character. The synthetic agent returns the model label `CobaltReady` in `session/new`. Its first appearance in the terminal is the ready milestone. After both the typed character and the model are visible, the driver submits the draft and requires an acknowledgment from the agent in the same session.

The provider probe opens `/resume` inside the active plain TUI. It measures from command submission to the first saved row and from Down+Enter to restored assistant content and the latest reply. Every turn supplies a user message and bold Markdown in the assistant reply. The probe checks that each numbered reply reaches the PTY in order and that a fresh prompt reaches the restored session. This route exercises the provider inventory. The alternate-screen attach measurements exercise the host session list and host event journal.

Both probes use a 120×40 controlling PTY, continuously drain its master, and send the terminal query replies from #764. Timings end when the corresponding bytes reach the PTY reader. Terminal emulator painting and display scanout add their own costs. Each binary gets one untimed warmup. Subsequent samples rotate binary order. All per-run JSON times use milliseconds from process launch. Compute paired differences within each sample for milestone intervals.

`agent_enter` includes interpreter startup before the first fixture instruction. `initialize_rx`, `session/new_rx`, and `session/load_rx` are observed inside the agent. The corresponding `_reply` times precede flushing the response. The interval to the matching visible marker includes response transport, application event processing, terminal formatting and output. The optional diagnostic enables host-runtime debug logs, reporting first dispatch by method as `first_host_*` when those events exist. It aligns the host's UTC logs to the parent's monotonic clock at launch. Published timings use tracing disabled except for that dedicated diagnostic. All tests use warm binary and OS file caches, newly initialized homes and a local Python agent.

## Observed on Linux x86-64

These warmed runs compare the published 0.30.0-next.6 Linux binary (source `b999c713`) against the optimized stack at `96e84746`. Binary SHA-256 values and kernel details are recorded with the activation and provider samples. Each cell below is the median of per-sample elapsed times in milliseconds, with the observed range in parentheses. Release and stack identify the executable running both the frontend and the local synthetic session.

| Observation | Samples per binary | Release | Stack |
|---|---:|---:|---:|
| Launch to handled input | 10 | 57.4 (39.2–70.9) | 51.6 (37.6–74.8) |
| Launch to visible initialized model | 10 | 78.0 (64.1–92.6) | 82.8 (62.6–104.8) |
| Agent session/new reply to initialized model on screen | 10 | 4.6 (3.5–5.8) | 7.3 (6.1–9.9) |
| Provider list, 4 rows | 5 | 2.3 (1.3–3.1) | 22.1 (20.7–24.2) |
| Select to first restored content, 4 turns | 5 | 26.0 (24.9–28.9) | 9.3 (8.0–10.5) |
| Select to latest restored reply, 4 turns | 5 | 26.7 (25.3–29.1) | 9.5 (8.5–11.6) |
| Provider list, 128 rows | 5 | 5.5 (4.9–5.8) | 26.5 (25.0–27.0) |
| Select to first restored content, 128 turns | 5 | 48.3 (43.4–49.8) | 43.0 (40.0–47.8) |
| Select to latest restored reply, 128 turns | 5 | 69.3 (61.7–71.6) | 56.3 (51.9–59.8) |
| Submit to first streamed reply | 5 | 52.8 (52.2–53.1) | 53.3 (52.2–53.5) |
| Character typed during stream to visible draft | 5 | 1.2 (1.2–1.4) | 1.2 (1.2–1.3) |
| Character typed after final output to visible draft | 5 | 1.2 (0.3–3.2) | 1.2 (1.2–1.2) |

The stack starts the provider before showing the saved-session picker. With four rows its second agent enters a median 18.6 ms after command submission. On the release it enters 23.3 ms after submission, after the user has selected a result. In every provider run the terminal received the entire sequence of uniquely numbered replies and the latest marker. The 128-turn sequence completed in 69.2 ms on release and 56.2 ms on stack.

In three separate traced stack starts, the owned host dispatched prepare a median 34.8 ms after launch. Another 18.5 ms passed before the agent received initialize. From host session/new dispatch to agent receipt took 0.24 ms, and writing the agent's reply to seeing the model took 5.91 ms. The first interval includes frontend and host startup, and the second includes process launch and the Python interpreter. The experiment cannot attribute these combined intervals entirely to transport.

The unmodified #764 gate ran twice with 20 observations per series. The stack's cold-host p95 exceeded the computed ceilings both times, at 59.90 versus 59.43 ms and 59.16 versus 58.82 ms. Both stack medians were near 58.2 ms. The cold-start readiness loop in `nori-rs/cli/src/host/start.rs` polls every 50 ms on this baseline. Detached host startup is also the subject of separate lifecycle PR #765. First input p95 passed on the first run (64.39 ms) and narrowly crossed the second run's ceiling (67.06 versus 67.04 ms). The first-frame p95 also crossed the second ceiling. The empty warm attach median was 6.3 and 6.2 ms on stack. Full gate values, two release control series, and raw timings are in `evidence/startup-764*.json`.

The additional live case uses the plain TUI with 40 synthetic Markdown chunks at 10 ms intervals. It types β after seeing the first reply, verifies that the draft character appeared before the agent finished streaming, then types ζ after seeing the final output and waits for the new draft to paint. Reproduce using the startup-ready command above with `--samples 5 --live-chunks 40`. All raw results accompany this file under `evidence/startup-*.json`.

## Final fixed binary compared with its stack baseline

The final validation reran #764 with the stack binary in both control slots and the fixed binary as candidate (20 samples per slot). All eight median and p95 checks passed. The fixed median was 38.70 ms to first handled input with initialization held open, 58.11 ms to start a detached host, and 6.21 ms to reach its empty warm attach list. Its cold-host p95 was 58.64 ms against a 59.87 ms ceiling. The earlier release-to-stack tail misses are recorded above.

Five interleaved samples per binary exercised responsive input, activation, provider restore and live streaming with the same terminal and agent. Ranges in parentheses are the observed minimum and maximum, in milliseconds.

| Observation | Stack | Fixed |
|---|---:|---:|
| Launch to handled input | 39.0 (36.4–70.7) | 39.9 (36.4–58.3) |
| Launch to visible initialized model | 63.1 (58.9–91.7) | 64.5 (58.7–80.0) |
| Provider list, 4 rows | 20.9 (20.6–21.3) | 20.7 (20.3–21.0) |
| Select to first restored content, 4 turns | 8.3 (7.9–9.5) | 9.1 (7.9–9.4) |
| Select to latest restored reply, 4 turns | 9.2 (8.5–9.8) | 9.4 (8.1–9.6) |
| Provider list, 128 rows | 23.7 (22.8–24.0) | 23.4 (23.0–24.0) |
| Select to first restored content, 128 turns | 41.0 (35.4–42.6) | 38.6 (34.3–42.3) |
| Select to latest restored reply, 128 turns | 54.0 (46.3–54.3) | 48.5 (45.5–54.0) |
| Submit to first streamed reply | 52.8 (52.3–53.1) | 53.3 (52.6–54.1) |
| Character typed during stream to visible draft | 1.2 (1.2–1.2) | 1.2 (1.2–1.2) |
| Character typed after final output to visible draft | 1.2 (1.2–1.2) | 1.2 (1.2–1.2) |

Every saved reply appeared in order and the restored session received the next prompt. The fixed binary SHA-256 is `2227787aab425c0fde18bce12b95240ae86feeaaf4f1fac736e730a061e9de2d`. Reproduce with the same commands above using the stack and fixed binaries as the pair, and five activation samples. Raw reports are in `evidence/startup-fixed-*.json`.
