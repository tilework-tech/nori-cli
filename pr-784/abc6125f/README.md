# PR #784 proof at `abc6125f`

- Binary: `nori-rs/target/debug/nori` built from `14e02d88`. `14e02d88..abc6125f` changes only tests, the perf script and docs, so the product code is identical.
- Grid: 120×40. Rendered with ghostty-web 0.4.0 via `tui-capture-with-ghostty-web`. Each PNG was re-rendered from its `screen.ansi` without the final newline.
- Data: synthetic only. Agent `mock-model`. Throwaway `NORI_HOME` under a scratch folder, with `project/` trusted. `NORI_NO_ANALYTICS=1`.
- Host: `nori host --port 0`, then `nori host new`. Terminals A and B run `nori attach <ID>`, some through `scripts/count_relay.py`, which logs every JSON-RPC method the terminal sends.

| Capture | Terminal | Keys | Proves |
|---|---|---|---|
| `01-watcher-ctrl-c-esc` | B (watcher) | type a draft, Ctrl+C, Esc | Refusal names Alt+T; draft kept; still `watching`; A's turn still running |
| `02-controller-still-streaming` | A (controller) | none | A's `mock:stream-until-cancel` turn keeps running after B's keys |
| `03-joined-pager` | B | PgUp | Joined transcript pager footer offers only `q/esc to quit` |
| `04-controller-interrupted` | A | Ctrl+C | The controller interrupts its own turn |
| `05-first-prompt-once-controlling` | B, fresh host | typed at 30 ms/key as soon as `controlling` shows | First prompt reaches the host and is answered |

Request counts with real terminals (`two-terminal-counts/`):
- `ctrl-c-pair.txt`:
  - Watcher B sent 0 requests (`list` heartbeats excluded) across Ctrl+C, Esc, Ctrl+C, Esc.
  - Controller A's first Ctrl+C cleared its draft, as it already did for its own prompt. Its second sent exactly one `_nori/session/cancel_observed`. A Ctrl+C on an empty idle prompt detached.
- `first-prompt-fresh-host.txt`: 3/3 runs. The first prompt typed at key speed as soon as the screen read `controlling` produced exactly one `_nori/session/prompt` and a reply.
- `first-prompt-after-alt-t.txt`: 3/3 runs with the same result after Alt+T.
- An instant `tmux send-keys "text" Enter` burst is read as a paste in any terminal, including plain `nori`, so its Enter does not submit. That is the #785 report's "disappearing" first prompt.

Replay (release, Linux, 64 sections, 5 samples, `--live --stream`), medians, on the same box:

| Metric | head | `c8f3ee3b` |
|---|---|---|
| launch → list | 45.9 ms | 49.2 ms |
| selection → first content | 10.9 ms | 13.3 ms |
| selection → latest reply | 60.6 ms | 59.1 ms |
| PageUp | 4.7 ms | 5.4 ms |
| key echo when settled | 4.8 ms | 5.4 ms |
| key echo while streaming | 4.8 ms | hung (PageUp opens the pager) |
| 24-chunk stream total | 474 ms | n/a |
