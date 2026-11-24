# Mixed host and attach versions at #752 `b959a6ea`, 2026-11-24

Answers the open question in #752 decision 3 with observed behaviour instead of
source reading. Old = #751 `5cb13fcd` (sha256 8610b2a9…), new = #752 `b959a6ea`
(sha256 13cebf21…). Both built from clean worktrees; a no-op rebuild confirmed
each binary matches its head. Linux, tmux PTY, synthetic agent servers.

`run.py --nori <host build> --client <attach build>`. The host and every helper
use `--nori`; only the attach terminal under test uses `--client`.

| Scenario | What it does |
|---|---|
| S38a | `nori host new --prompt "first errand"`; the client under test attaches; a second terminal on the host's build takes control and picks Ask, then prompts |
| S38b | the client under test runs `attach`, `/new`, and prompts |
| S38c | as S38a, but the client under test takes control and picks Ask itself, then reopens `/approvals` |
| S39a/b | host advertises modes for an opencode session (no Ask); compared with the attach and plain `/approvals` menus; then Ask is chosen |

| Host ← client | Checks | Result |
|---|---|---|
| new ← new (control) | 34/36 | everything passes except S39 menu = host list (decision 2) |
| old ← old (control) | 19/24 | failures are only "mode change" checks: old attach has no `/approvals` |
| **new ← old** | 8/24 | **broken**: see below |
| old ← new | 20/24 | works; mode change applies but the client cannot show it |

## Old client on the new host (`results/mixed/host-b959a6ea_client-5cb13fcd`)

- Stays connected and keeps running; role changes still arrive.
- On joining or creating any session, prints the raw serde error
  `unknown variant approval_mode_changed …` and `Replay gap … expected event 4,
  received 5`. The host emits the event at every session start, so every session
  is affected.
- Shows no conversation history (S38a: `heard: first errand` never drawn).
- Refuses every prompt: `Session history has a replay gap. Resolve it before
  sending more work.` The prompt stays in the composer; the adapter ledger shows
  0 `session/prompt` (S38b, S38c).
- After another terminal changes the mode and prompts, the old client's view
  stays frozen: no reply, no mode label (S38a).
- The PR text "fail on `approval_mode_changed`" is accurate but understates it:
  an older `nori attach` cannot use any session on a `b959a6ea` host.

## New client on the old host (`results/mixed/host-5cb13fcd_client-b959a6ea`)

- Connects, shows history, `/new` and prompts work, exactly one prompt each.
- Picking Ask reaches the host: the ledger shows `session/set_config_option mode
  default`; the observer reads `ask`.
- The header shows no `Approvals:` label, and reopening `/approvals` marks no row
  `(current)` and highlights Agent while the host is in Ask (S38c
  `3b-menu-after-change`). Compare the same step on the new host, which shows
  `Approvals: Ask` and `Ask (current)`.
- S38a's "other" changer is the old attach build, which has no `/approvals`, so
  that mode-change step is not applicable for an old host.

## Agent without Ask (S39, new host)

- Host advertises `availableModes` and the `mode` config option as
  `[agent, approve-all]` for opencode, `[ask, agent, approve-all]` for claude-code.
- Both `/approvals` menus (attach and plain) list Ask anyway.
- Choosing Ask: host refuses, screen shows "opencode has no mode that asks before
  every edit and command", host mode stays `agent`, no adapter mode change.
