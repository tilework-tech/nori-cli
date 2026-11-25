# Independent acceptance: #778–#785 on real `nori attach` (restacked heads)

All data is synthetic. No feature branch was edited. The heads under test were published at 20:15–20:21 UTC:

| PR | Head | Result |
|---|---|---|
| #778 queue recovery | `e1a1e4b4` | Own-prompt take-back works exactly once, including after reconnect. One owner decision (D1). |
| #779 prompt history | `323bd18c` | Pass. L1 from before still open. |
| #780 custom prompts | `c9b0f41c` | Pass |
| #781 goal | `8c9af580` | Pass (it failed in attach at the earlier `db931955`) |
| #782 context | `e3c18ca1` | Pass. Its L1 (the path is cut so the file name is hidden) is still open. |
| #783 compact | `f4676ee8` | Pass |
| #785 shell | `ef209c3e` | Fails Cliff's local-only rule: a remote controller's `!` runs on the host. Patch `da3f2a53` (not applied yet) passes. |
| **Whole stack** | | **Blocker B1: built on #784's old commit `c9b383ca`, not `c8f3ee3b`.** |

Builds: release (LTO off) of `ef209c3e` and of the shell patch `da3f2a53`. `da3f2a53` adds one commit on top of `ef209c3e`, so the R and probe results below hold for both except where noted. Each feature was checked at the top of the stack, not at its own head. The stack is linear and each later commit leaves the earlier features' code alone, but nothing was built at the intermediate heads.

Evidence: `feat-785-ef209c3e/` (R scenarios, probes), `feat-785-da3f2a53/` (shell, probes, stale-base, typeahead, perf). `feat-785-a53c5fd7/` is the pre-restack run, kept for history.

## B1 (blocker): the stack sits on #784's earlier commit

`e1a1e4b4`'s parent is `c9b383ca`. It lacks `c8f3ee3b`, "page up through attach history and open the picker at once". On every head from #778 through #785, `nori attach` therefore:
- **Loses typed input.** R03 typeahead fails 3/3 at `c9b383ca`, `38f82990`, `a53c5fd7` and `ef209c3e`; it passes 3/3 at `c8f3ee3b` and main (`feat-785-da3f2a53/typeahead/`).
- **Pops the picker over the session about 1 s after launch, and the picker takes the keys.** After `/new`, the first prompt `fresh errand` lands in the picker's search box as `resh errand` and never reaches the agent (`stale-base/S38b/2-after-prompt.png`). S06, S07, S10, S12, S13, S18b, S37d and S38b fail the same way at `da3f2a53` and at `c9b383ca`; all pass at `c8f3ee3b` (`stale-base/*.json`).
- **Opens saved sessions 1 s late.** First content at 1,079–1,089 ms vs 86–95 ms at `c8f3ee3b` (`perf/joined_open.json`).

Fix: rebase #778 onto `c8f3ee3b` and restack #779–#785.

## #778 `e1a1e4b4`

The new `capabilities.nori_sessions.composer` marks a client's queued prompts as typed by its own user.
- **R05a, own take-back once (pass).** Terminal A queues `relay 1 lamp` during a turn and interrupts. Up then Enter puts it back in A's composer, the queue row disappears, and it is never sent.
- **R05b, after reconnect (pass).** Same as R05a, with A's relay cut for 3 s before the take-back (`R05b/reconnected.png`, `reclaimed.png`).
- **Wire (`probes-queue-composer.json`).** A composer client takes its text once, then gets `null`. After reconnecting it takes the next entry once. A client without `composer` queues prompts that stay non-editable: take returns `null` and resume delivers all three once, in order.
- **D1 (owner decision): another terminal can take your prompt.** Terminal B takes control with Alt+T and presses Up: A's queued text goes into B's composer (R05c, `other-reclaim.png`). On the wire, a second composer client and even a client without `composer` can take a composer-marked prompt once they control the session. The host remembers which kind of client queued a prompt, not which terminal. The earlier coordinator reading was "a controller takes back its own queued text … preserve the distinction between that terminal's prompts and prompts another client submitted". Either keep the current rule (any controlling terminal may take any typed prompt) or tie take-back to the terminal that composed it.

## #779–#783 on real attach (R06–R10, all pass)

- **R06 #779.** Terminal A sends `relay 1 cormorant`. Terminal B is remote (`--host`), has its own `NORI_HOME` and takes control. In B, Up recalls the prompt and Ctrl+R finds it. It is stored once in the host's `history.jsonl`, and the client home is untouched.
- **R07 #780.** The remote terminal lists the host's `harbor-review` prompt, not its own `laptop-decoy`. The markdown prompt expands and is sent once. The script prompt runs on the host in the session's folder (`ran-in-beta`).
- **R08 #781.** The controller sets `/goal chart the north harbor`. The watcher shows the goal card. The watcher's `/goal sink the fleet` is refused.
- **R09 #782.** `/context` from the remote terminal prints the report and lists the session folder's instruction file. **L1 is still open:** the row reads `…/roots/beta/...  ~10`, so `CLAUDE.md` is cut off.
- **R10 #783.** A watcher's `/compact` sends nothing. The controller's `/compact` finishes, and both terminals show it. The session keeps its ID, and the next prompt is delivered once.
- **Wire, `da3f2a53`: 85/87 pass.** Compaction 9/9: the watcher gets `-32016`, the summary is requested, `context_compacted` reaches the watcher once and a late joiner's replay, and an unknown session gets `-32002`. The 2 failures are D1 and #779 L1 (`history/add` accepts an unknown session).
- **S34m:** `nori resume --last` still sends `_nori/host/history/metadata` and `_nori/host/prompts` (#779/#780 L2 from before).

## #785: Cliff's local-only `!`

| | `ef209c3e` (published) | `da3f2a53` (patch) |
|---|---|---|
| Local attach: `!` runs in the session folder (`beta`), output shown; a watcher sees the `Ran` cell | pass | pass |
| Watcher `!touch …` runs nothing | pass | pass |
| Remote controller (`--host`, own home, after Alt+T): gray hint "Remote shell commands are not yet supported." | **fail** (it ran) | pass (`38;5;8`, not red) |
| Remote: nothing runs on either machine, nothing goes to the agent, the command returns to the `!` prompt, and the local terminal gets no new `Ran` cell | **fail** | pass |
| Wire: local `initialize` advertises `shell: true`, remote `false`. A remote controller gets `-32018` (also for an unknown session). A wrong local token connects as remote. `host/local-token` is 0600 and appears in no other host file. Empty command `-32602`. | – | 14/14 (`probes-shell.json`) |

Once `da3f2a53` is pushed to `feat/host-session-shell` as a fast-forward, the head SHA is the same and this verdict applies unchanged.
