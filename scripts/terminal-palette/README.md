# Measuring terminal palette startup

These Linux probes start optimized binaries on a 120×40 controlling PTY. They seed two ready sessions on a local host using one section of synthetic Markdown, one completed tool and a filler conversation. Plain `nori` runs a synthetic ACP process. `nori cloud` runs the same process behind a local `NORI_HANDROLL_BIN`. The explicit managed attach endpoint uses loopback.

The PTY continuously drains output and answers cursor/device queries. In truecolor mode it answers OSC 10 and 11 immediately, 150 ms after each query, or never. The first keystroke is sent when the first palette query arrives or the first frame appears, whichever occurs first. Cloud input is sent when its inventory appears. Its synthetic provider advertises three saved sessions. Echo is disabled, so a draft or picker query must reach application output to count. The direct attach adapter checks the exact two typed characters. The application must render another key after focus, restore terminal attributes on exit, and balance alternate-screen transitions. The `host ls` sample queues shell typeahead before the one-shot process finishes. Piped output uses stable session IDs and contains no terminal control sequences.

Ready means a populated picker, the last saved tool for direct attach, or an initialized model for plain startup. The piped-prompt case verifies a reply to the piped text and a reply to the subsequent draft. The 120 ms pause before plain/direct submit lets the paste-burst detector settle and is excluded from first handled input. The delayed-palette check observes a light background-relative row stripe and a dark stripe after focus changes. Timings end when bytes arrive at the PTY reader. The separate tmux proof uses a terminal emulator.

## Results

Measurements used warm optimized Linux x86-64 musl binaries on one Modal host (kernel 7.3.4), fresh homes and local synthetic agents. Each cell reports baseline → repaired median milliseconds. Five rounds rotated the cases and alternated the baseline and candidate within each round. Each binary ran 150 primary cases, 12 diagnostic cases and six editor/job-control cases. The raw observations are in [samples.csv](samples.csv) and [handoffs.csv](handoffs.csv). Every `ms_` column starts at process launch. The independent responding-terminal startup series is in [startup-764.json](startup-764.json).

**Launch to handled input.** For one-shot host listings this is launch to populated output.

| Entry point | Immediate | 150 ms per reply | No palette reply |
|---|---:|---:|---:|
| Attach picker | 8.2 → 7.1 | 308.3 → 7.1 | 2,007.5 → 7.1 |
| Attach by ID | 7.6 → 7.1 | 307.6 → 7.1 | 2,007.5 → 7.1 |
| Explicit attach endpoint | 7.7 → 7.1 | 307.9 → 7.0 | 2,007.7 → 7.0 |
| Saved-session picker | 37.0 → 35.8 | 337.2 → 36.8 | 2,037.1 → 37.5 |
| Fresh inline composer | 39.2 → 37.5 | 340.5 → 61.2 | 2,057.9 → 49.3 |
| Inline after a piped prompt | 28.4 → 27.3 | 330.4 → 28.3 | 2,029.0 → 29.5 |
| Cloud picker, synthetic adapter | 59.4 → 84.2 | 383.4 → 61.0 | 2,069.5 → 84.7 |
| Host list on a tty | 7.1 → 7.1 | 307.0 → 6.9 | 2,007.0 → 7.0 |
| Host list piped | 6.2 → 6.0 | — | — |

On a terminal that withheld its colors, the five-run ranges for first handled attach input were 2,007.4–2,008.8 and 6.8–8.8 ms, respectively. The published 0.30.0-next.6 Linux executable reproduced the sequence in three trials per mode. Its medians were 7.1 ms with immediate replies, 307.6 ms at 150 ms per reply and 2,007.8 ms with no replies.

**Ready and subsequent input.** The next-key interval starts when the probe injects that character after the ready frame. The focus interval starts with a real focus-gained notification followed by a typed character.

| Entry point | Ready, no palette reply | Next key after ready, no reply | First key after focus, delayed replies |
|---|---:|---:|---:|
| Attach picker | 2,009.4 → 8.7 | 0.4 → 0.4 | 301.5 → 0.7 |
| Attach by ID | 2,031.8 → 31.7 | 1.7 → 1.3 | 302.9 → 2.0 |
| Saved-session picker | 2,037.1 → 37.5 | 1.4 → 1.3 | 302.6 → 2.0 |
| Fresh inline composer | 2,082.4 → 74.7 | 1.2 → 1.2 | 302.0 → 1.3 |
| Inline after a piped prompt | 2,054.8 → 54.4 | 0.5 → 0.3 | 302.0 → 1.2 |
| Cloud picker, synthetic adapter | 2,067.5 → 83.0 | 1.3 → 1.4 | 302.2 → 1.5 |

With background-only replies on fixed attach, the first key was handled at 7.5 ms, the emulator wrote OSC 11 at 156.5 ms, and the light background stripe was visible at 157.3 ms. Changing the synthetic background to dark on focus changed the rendered stripe again. The baseline waited on OSC 10 and issued no background query.

**Terminal handoff.** Every ordinary 150 ms delayed-reply sample returned the full next command on attach, saved-picker and full-screen exit when typed at the first alternate-screen leave sequence. The one-shot table left shell typeahead untouched and used terminal-default row backgrounds. Each of three Ctrl+G editors and three suspended foreground jobs received its own input without terminal response bytes. Nori handled the next key on return.

Exiting attach immediately after startup kept the original screen until pending responses were drained: median 147.8 ms from quit to shell visibility with 150 ms replies, and 198.3 ms when the emulator also ignored the DSR status barrier. The wait uses the remaining 200 ms query window. With intentionally late 450 ms replies, all five exits handed over before the two OSC replies and the DSR reply arrived. The new owner received those three control messages alongside its intact typed command. A PTY has no mechanism to revoke a query already received by the emulator.

**Independent startup limits.** With every query unanswered, both builds failed on the mandatory cursor-position read before issuing a color query. Repaired attach exited in 2,026.6 ms and restored the original terminal attributes, including canonical input. The baseline left the terminal in raw mode. A separate keyboard-capability query still costs about two seconds when the terminal answers cursor and colors but withholds device attributes.

**PR 764.** With 20 samples per slot, 7 of eight original median/p95 gates passed. Median baseline → candidate timings were first composer frame 47.2 → 41.4 ms, first handled input 48.6 → 42.6 ms, start detached host 59.2 → 59.3 ms, empty warm attach 7.4 → 7.2 ms. The empty warm-attach p95 was 8.51 ms against an 8.27 ms ceiling. On a second complete run it passed at 7.20 ms against 8.32 ms. The sole crossing on repeat was cold detached host p95 at 59.68 ms against 59.62 ms. Detached host startup does no palette I/O. The [repeat samples and gates](startup-764-repeat.json) retain this variation.

The baseline is the complete source tree of `8099204975768d0b0d17e7bd21f13cfbd48ddb59` (binary SHA-256 `4b7fe6aa295fa529df92a6d55bd0abbe63cca04f7de6d31e2215c7825ec6e38d`). The candidate source was local commit `5d03ec6c9`; its production tree was pinned for measurement. The candidate binary SHA-256 was `bbc55a1547dc4e07f4223a00654144e9bbaa4f6f849672e228148bb76aa85845`.

## Reproduce

Use Python 3.11 or newer and a local clone with the pinned benchmark commit from PR 770. Extract its original fixtures without switching branches. Build the base and candidate separately with Rust 1.90.0, retaining each binary. Run from the repository root.

```sh
git fetch origin pull/770/head
mkdir -p /tmp/nori-palette-fixtures
git archive 789a2ce53f5082553395ce75813ec248934e429c scripts/attach_perf |
    tar -x -C /tmp/nori-palette-fixtures
(cd nori-rs && cargo +1.90.0 build --release --locked --target x86_64-unknown-linux-musl -p nori-cli -j 4)
python3 scripts/terminal-palette/measure.py --fixtures /tmp/nori-palette-fixtures/scripts/attach_perf \
    --base "$BASE" --binary "$FIXED" --samples 5 --check --output palette-comparison.json
```


The separate foreground handoff probe launches a real external editor process with Ctrl+G. It also runs attach as an interactive Bash job, suspends it with Ctrl+Z and resumes with `fg`. Each owner reads an input line from the controlling terminal and reports any terminal replies mixed with its own bytes. The resumed Nori must handle another typed character.

```sh
python3 scripts/terminal-palette/handoffs.py --fixtures /tmp/nori-palette-fixtures/scripts/attach_perf \
    --binary "$FIXED" --samples 3 --check --output palette-handoffs.json
```

The historical eight-gate startup benchmark uses responding terminals and holds ACP initialization open. Run it separately for a fresh inline composer, first handled input, cold detached host and empty warm attach picker.

```sh
git fetch origin pull/764/head
git show 5cff62342dcfe2583172d80fdb451cf3ab7eff00:scripts/host_startup_benchmark.py > /tmp/nori-startup-764.py
python3 /tmp/nori-startup-764.py --base "$BASE" --head "$FIXED" --samples 20 --output palette-764.json
```

To isolate a terminal that answers only the background query, or a terminal that ignores even the required cursor query, select the diagnostic modes explicitly.

```sh
python3 scripts/terminal-palette/measure.py --fixtures /tmp/nori-palette-fixtures/scripts/attach_perf \
    --base "$BASE" --binary "$FIXED" --paths attach plain --modes background-only silent-all device-silent \
    --samples 3 --check --output palette-controls.json
```
