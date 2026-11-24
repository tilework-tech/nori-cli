# Startup prompt lost on native macOS (#750 `32471d77`, shared by #751 `c31726f7`)

All data is synthetic (fixture agents Amber/Birch, generated homes).

## What the native trace shows

`verify/pr-750-32471d77-native-trace` repeats `host_startup` + `host_new_session`
8 times on native macOS with `nori_tui`, `nori_harness`, `nori_acp_host` and
`nori_host_runtime` at trace level (run 45394564103). Round 6 failed
`startup_activates_its_session_through_the_owned_host`
(`native-round6-failure.txt`):

| Time (UTC) | Layer | Event |
|---|---|---|
| 12:35:58.950 | host | `_nori/host/prepare_agent` |
| 12:35:59.003 | host | `session/list`, `session/new` |
| 12:35:59.005 | harness | `ACP session created: SessionId("0")` (adapter answered) |
| 12:35:59.056 | tui | `received 0 custom prompts` (new widget is live) |
| ~12:35:59.05 | test | types `whoami` (after the host logged `session/new`) |
| ~12:35:59.95 | test | presses Enter 900 ms later |
| 12:35:59.978 | tui | `paste_handling: ERR: No such file or directory` (a paste-burst flush tried `whoami…` as an image path) |

The final screen shows `› whoami` in the composer, not in the transcript: the
cursor sits on the next composer row. The terminal never submitted the prompt,
so neither the host nor the adapter saw a `session/prompt`. The host and adapter
did not drop it.

## Mechanism

`whoami` arrives as a fast run of characters, so the composer holds it in its
paste-burst buffer. A `Draw` tick normally flushes that buffer about 8 ms later
(`ChatWidget::handle_paste_burst_tick`). In the failing round no flush happened
in the 900 ms before Enter, so Enter found the burst still active and was
appended as a newline (`chat_composer/key_handling.rs`, the
`paste_burst.is_active()` branch before submission). The flush that followed
treated the text as a pasted path. The implementer's own trace
(`debug/pr-750-startup-trace`, run 45393081099) agrees: at Enter the composer
text is empty with `burst=true` on the newly installed widget.

The open question for the fix is why no flush tick ran for ~0.9 s right after
activation on macOS. Either the event loop was busy, or `Draw` events were
handled on a path that skips the burst tick (for example
`handle_tui_event` routes `Draw` to the backtrack overlay while
`self.overlay.is_some()` and never calls `handle_paste_burst_tick`). A person
typing in that window loses Enter the same way, so this is not only fixture
timing.

## Linux

Not reproducible on Linux:
- 40 repo-test runs at #750, including 32 busy-loop workers (`loop-race.sh`)
- S36: 36 trials, Enter swept around the `session/new` reply
- S36b: the burst typed 3–50 ms after the reply
- the whole `nori` process paused with SIGSTOP while typing and pressing Enter,
  then resumed (all delivered: `*-stall/`)

## #751

#751 does not touch this path (its composer diff only changes slash-command
dispatch). Its traced repeat passed 8/8 (`native-751-results.txt`) and the
implementer's repeat 50/50, but its full native run failed the same test twice
(45384170358). Treat it as affected until the #750 fix is restacked.
