# Independent acceptance: #789 at d98c3e1b (local-only `!` commands)

All data is synthetic. No feature branch was edited. Base `main` 66264b5d (the attach stack has landed). Release builds of `d98c3e1b` and of `main` as the control.

## Verdict: pass on Linux and native macOS

| Check | Linux `d98c3e1b` | macOS 15.7 arm64 `d98c3e1b` | Linux `main` (control) |
|---|---|---|---|
| R13: plain `nori` launched in `beta`, `!echo … && touch` prints `plain-in-beta`, a marker file appears once in `roots/beta/app`, and nothing is sent to the agent | pass | pass | pass |
| R11 local: `nori attach` from the `alpha` folder, session in `beta`. `!` output is `shell-in-beta` and the marker is in the session folder only | pass | pass | pass |
| R11 watcher: the `Ran` cell shows; `!touch` runs nothing; the command stays in the prompt | pass | pass | pass |
| R11 remote (`--host`, its own `NORI_HOME` and folder, after Alt+T): gray hint "Remote shell commands are not yet supported." (`38;5;8`, not red), `!touch shell-canary-remote` kept in the `!` prompt | pass | pass | **fail** (ran) |
| R11 remote: no marker anywhere in the run (host home, client home, every root), nothing sent to the agent, no new `Ran` cell in the local terminal | pass | pass | **fail** |
| Wire, 14/14 (`probes.json`): local `initialize` advertises `shell: true`, remote and wrong-token clients get `false`. A remote controller gets `-32018` with nothing run and no `user_shell` event, including for an unknown session. Local watcher `-32016`, empty command `-32602`, local unknown session `-32002`. `host/local-token` is 0600 and in no other host file | 14/14 | 14/14 | – |
| R01/R02 (reply order, picker and `/new`), as a regression guard | pass | pass | – |

- Scenarios: 27/27 on Linux and macOS.
- macOS ([run 45715403588](https://github.com/tilework-tech/nori-cli/actions/runs/45715403588), branch `verify/pr-789-d98c3e1b-native-acceptance`, pinned to this head): attach PTY rounds 108/108.
- macOS full PTY suite: 1,974/1,974 on the second attempt. The first attempt had one failure, `prompt_flow::test_multiline_input` (timed out waiting for "Line 1"), which is unrelated to `!` and passed on the rerun.
- The PR's own PTY test `joined_shell.rs` is `#![cfg(target_os = "linux")]`, so on macOS the remote case is covered only by this harness.
