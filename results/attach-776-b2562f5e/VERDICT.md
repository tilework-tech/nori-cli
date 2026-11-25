# Independent acceptance: #776 at b2562f5e (join a running host from the shared client)

All data is synthetic (lantern adapter, generated homes and roots). No feature branch was edited.

| | SHA |
|---|---|
| #776 head | `b2562f5e` (base `main` 0466dddf) |
| Baseline | `main` 0466dddf |

Builds: `cargo build --release --locked -p nori-cli`, Rust 1.90.0, Linux (Modal, kernel 7.3.4, tmux 3.3a), one `CARGO_TARGET_DIR` per revision.

## Verdict: pass, no blocker

`HostLink::join` has no CLI caller at this head, so there is no user-visible change to accept yet. The library layer and both existing entry points pass.

## Library layer (verifier-only tests, against a real `RemoteAcpServer`)

`harness/verifier/host_link_verifier.rs`, injected by `harness/verifier/inject.sh` into a verifier tree only. All 5 pass:

| Test | Checks |
|---|---|
| v1 | Control moves A→B→A three times; each side sees the other's change |
| v2 | A call made while disconnected is refused within 500 ms ("Reconnecting…"); Alt+T while down sends nothing later |
| v3 | A controller that reconnects after another terminal took control comes back as a watcher; the new controller is not demoted |
| v4 | An idle joined link stays connected for 14 s, past the host's 10 s silence timeout |
| v5 | Leaving closes only that connection; the other terminal is still answered, can take control, and a new terminal joins as a watcher |

`cargo test -p nori-tui`: 1,946 passed, 0 failed (the PR's 1,941 plus these 5).

## Entry points (regression)

Full S01–S45 matrix with real `nori attach` and plain `nori`, compared check by check with `main`: **no check regressed**. All three revisions share the same 39 failures that predate the stack (listed in `results.json`). S24 is inconclusive on every revision because the harness can't read `/proc/<pid>/fd` on this machine.

## Not covered here

- Native macOS: deferred to the launcher head, as agreed.
- From reading only, unverified: when the host refuses Alt+T, `HostLink` produces no update, so the terminal shows nothing.
