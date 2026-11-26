# Independent acceptance: #793 at f59783d9 (history only for sessions the host knows)

All data is synthetic. No feature branch was edited. `f59783d9` is one commit on `main` 66264b5d. Release builds (LTO off, 16 codegen units) of `f59783d9` and of landed `main` 85da3524 as the control, in one target dir checked out in sequence; the binary hashes differ.

## Verdict: pass on Linux and native macOS

| Wire check (`feature_probes.py --only history`) | Linux `f59783d9` | Linux `main` |
|---|---|---|
| `history/unknown_session_add_refused`: a well-formed unknown ID is refused | pass (`-32002`) | **fail** (`result: {}`) |
| A malformed ID gets `-32002` | pass | **fail** (`-32602`) |
| A watcher with an unknown ID still gets `-32016` first | pass | pass |
| Refused adds leave `entryCount` unchanged and write nothing to the host's `history.jsonl` | pass | **fail** (count 3 → 4, `ghost` written) |
| Valid host-wide control: three controller adds are recorded in order, listed by entry and found by search from a watcher; stored in the host home, client home untouched | pass | pass |
| A valid add after a refusal is recorded and searchable | pass | **fail** (count off by the ghost) |

- Probes: `f59783d9` 18/18 (`probes-history.json`), `main` 12/18 (`probes-history-main-85da3524.json`).
- R06 in the real UI (`scenarios/`): a remote terminal recalls with Up and searches with Ctrl+R across the host; the prompt is stored once on the host and never in the client home. 4/4.
- Linux CI on the head passes.

## Native macOS

macOS 15.7.4 arm64, [run 45726836466](https://github.com/tilework-tech/nori-cli/actions/runs/45726836466), branch `verify/pr-793-f59783d9-native-acceptance`, checked out at `f59783d9` (the `nori-rs` tree matches the head, `4300483d`). Harness `7503ab40`. Evidence in `macos/`.

- History probes 18/18, R06 4/4.
- `host_lifecycle` history wire tests: 11/11, including the PR's `a_prompt_for_a_session_the_host_does_not_know_is_not_recorded`.
