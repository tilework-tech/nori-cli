# Attach built-ins: independent interactive acceptance

Real `nori host` + `nori attach --host` binaries, driven in tmux PTYs against
Quill, a synthetic ACP agent server. Harness: `harness/attach-builtins-verify/`.
Protocol and message ordering are reviewed separately; this covers what a
person at the terminal sees and where their input goes.

## Tested heads

| PR | Head | Verdict |
|---|---|---|
| #802 `/agent` | `c352fb74` | **Changes requested**: watchers and attach-by-ID leave an in-place-changed session (finding 1) |
| #803 `/model` `/config` | `8c9ab9ae` | **Pass for its own scope**: both review cases fixed in the terminal. Inherits finding 1 from #802 |
| #804 `/mention` | `7c469a25` | **Pass** |

Baseline: main `214d79b0` (same harness, same synthetic agents).
Superseded heads, kept for comparison: #803 `61789f95` and `211a8f87`; #804 `e6690ffc` and `ef806945`.
`MATRIX.md` lists every check on every head. Where a check ran more than once, `×N` gives the run count and `p/n` gives passes out of runs.

## Findings

1. **Joined terminals leave a session whose agent changed in place, when the host has another live session** (#802, every later head).
   - A watcher already on the session jumps to the other live session after the change and never shows the changed session's later turns. W1 and W2 fail 3/3 on `c352fb74`, `8c9ab9ae` and `7c469a25` (`*/watcher-run/`). Proof: `proof/802-watcher-before-change.gif`, `proof/802-watcher-moved-to-other-session.gif`.
   - `nori attach <id>` for that session opens a different session. A7b and R2d fail on every head. With only one session on the host, attach by ID works (M5b).
   - Reaching the session through the Alt+S list works (R2e).
   - This contradicts #802's claim that watchers and later attaches keep the session open. The PR's own PTY test uses a single-session host.
2. **The controller's hint row keeps naming the previous agent after an in-place change** (#802, every later head).
   - Over a 12 s timeline (`inplace-mode-run*/inplace/m1-hint-timeline.txt`), the row still read `kestrel` at 22 s in 5 of 6 runs on `8c9ab9ae` and `7c469a25`. Across those runs and the `agent.sh` runs, it refreshed in 2 of 8.
   - It stays stale after the next prompt in most runs (M2c).
   - The next prompt always goes to the new agent in that agent's own mode (M2a and M2b pass in every run). The slash menu updates (A3e).
   - Proof: `proof/804-7c469a25-hint-row-names-old-agent.gif`.
3. **Native mode after an in-place change** (requested check).
   - On `8c9ab9ae`/`7c469a25`, the initiating controller shows the new agent's mode (M1b), follows the host receipt from `/approvals ask` (M3a), and fresh attaches show the current mode, both through the list (M4a) and by ID on a single-session host (M5a). The badge clears when the replacement has no mode (M6a).
   - On `211a8f87`, the controller showed no mode after the change (M1b fails). On `c352fb74`, the mode went stale after the receipt and on fresh attach (M3a, M4a and M5a fail).
   - Proof: `proof/803-211a8f87-inplace-native-mode-missing.gif` → `proof/803-8c9ab9ae-inplace-native-mode.gif`, `proof/803-8c9ab9ae-fresh-attach-native-mode.gif`.
4. **#803 review case: attach after a confirmed native mode change.** The late attach's badge shows `read-only` on `211a8f87` and later (R1f). Main and `61789f95` show the stale `workspace-write`. Proof: `proof/main-late-attach-stale-mode-badge.gif` vs `proof/803-8c9ab9ae-late-attach-mode-badge.gif`.
5. **#803 review case: same-ID replacement by an agent with zero options.**
   - Through the list, `/model` on the replaced session offers nothing on `211a8f87` and later (R2f). On `61789f95` it offered the old agent's models with `harbor-max (current)`.
   - The by-ID route can't be judged because of finding 1 (R2c ran against the wrong session).
   - The replayed welcome line still reads "moss options: Model=…", as the owner noted.
   - Proof: `proof/803-61789f95-replaced-keeps-old-models.gif` → `proof/803-8c9ab9ae-replaced-zero-options.gif`.
6. **Footer repeats "Approvals: Ask"** after `/approvals ask` in attach (R1e). This is also on main, so it predates this stack.

## What passes

- **#802:**
  - `/agents` is an alias and the slash menu lists `/agent` once.
  - The picker lists the host's agents and install state, not the client's.
  - From an empty host, choosing an agent starts a session with it.
  - Before the first prompt, the agent changes in place under the same ID.
  - After a prompt, the new agent starts in a new session beside the old one, which keeps answering after Alt+S.
  - An agent that isn't installed shows the host's reason, the picker stays open, and nothing starts.
  - Watchers get a greyed row with the take-control hint.
- **#803:**
  - A confirmed `/model` change reaches the next reply.
  - An agent's rejection is shown, and the picker and the agent keep the confirmed model.
  - `/config` lists only Model and Effort; an Effort change reaches the agent.
  - A 5 s delayed answer, sent before Alt+S, lands only in the session it was made in (B3a–d).
  - A terminal that attaches while the answer is pending sees it (B3e), and so does a later attach (B4a).
  - Watchers are greyed, and plain `nori` offers the same models.
- **#804:**
  - `/mention` with and without a session offers only host files, never the attaching folder.
  - Search follows a session in another host folder.
  - A watcher can mention.
  - An image match goes to the agent as a host path in a text block.
  - Plain `nori` inserts the same way.

## Mixed versions (`mixed/`)

| Client → host | Result |
|---|---|
| `7c469a25` (contract 1.10) → `61789f95` (1.9, serves the features) | Interactive path works: model, rejection, delay, late attach, native mode, zero-option replacement through the list, mention. Fails only on findings 1, 2 and 6. |
| `7c469a25` → `c352fb74` (1.9) | Same. The C4a failure there is a harness bug fixed later, not a product failure. |
| `7c469a25` → npm `0.30.0-next.3` (predates prepared agents and `find_paths`) | **Contract-gated path**: `/agent`, `/model`, `/config` and `/mention` are greyed "not available on this host yet", `/agents` isn't sent as a prompt, `@` never lists the client folder, and a prompt is delivered. 7/7 pass. |
| `7c469a25` → main `214d79b0` | Main's host already advertises prepared agents and `find_paths`, so nothing is gated. The G3 "failures" are a wrong expectation. |
| main → `7c469a25` | Prompt delivered; no client files. |
| `61789f95` (1.9) → `7c469a25` (1.10) | Works. The old client's stale mode and models remain, as expected. |

## Five-goal host audit (`audit/`)

`harness/audit/run.sh` from `nori/host-audit-five-goals-214d79b0`:

- Main: 23/23 findings observed, as recorded.
- `7c469a25`: 19 observed, 4 differ.
  - F3a, F3b and F3c are fixed (`/agents`, `/agent` and `/config` now work).
  - OK1 is a harness artifact: the audit script leaves the new agent picker open. Ctrl+D inside that picker is ignored; Esc then Ctrl+D detaches (`ctrl-d-agent-picker.sh`).
  - Every other audit finding (Ctrl+D in the session picker, watcher approvals, `host stop`) is unchanged.

## macOS (native, `blacksmith-6vcpu-macos-15`, aarch64, release build)

Workflows are pinned to the PR heads: `verify/pr-802-c352fb74-native-host` (run 46080953120) and `verify/pr-804-7c469a25-native-host` (run 46080964067). The #804 head contains #803 `8c9ab9ae`. Evidence is in `macos/`.

- **PR PTY suites, two rounds, Linux gate removed** (the suites are `cfg(target_os = "linux")`, so stock CI never runs them on macOS):
  - `c352fb74`: `attach_agents` 6/6 and `host_attach` 24/24 in both rounds.
  - `7c469a25`: `attach_agents` 6/6, `attach_mention` 2/2 and `host_attach` 24/24 in both rounds. `attach_config` passed 9/9 in round 1 and 8/9 in round 2: `an_agent_replaced_by_one_without_options_leaves_no_model_behind` timed out waiting for "This agent has no model selector". The screen showed the composer with no picker open, so `/model` never opened. Treat it as a macOS flake in the PR's test until it is repeated.
- **This harness on macOS** reproduces the Linux results: findings 1, 2 and 6 and the passes listed above.
  - B7a, C6a and C6b (plain `nori`) fail only on macOS. `/tmp` resolves to `/private/tmp`, so plain `nori` asks the trust and compact-output questions, which swallow the typed command. This is a harness environment issue, not a product failure.
  - On `c352fb74` the #803 and #804 rows fail because those features are not in that head.
- `macos/superseded-first-attempt/` holds the first attempt, which did not build the ci-test binaries; its PTY failures are spawn errors.

## Limitations

- Linux Modal VM, debug builds. The toolchain was Rust 1.90 with zig as the C compiler and the system OpenSSL, not the CI image.
- Synthetic agents only: no hosted providers and no real saved sessions.
- tmux timing: Quill keys are paced at 60 ms.
- Rows R1 and R2 on main and `c352fb74` are partly invalid, because `/model` is greyed there and the setup steps typed into the composer.
- R2c is not meaningful while finding 1 sends the attach to another session.
- A check that ran once can still be flaky. Repeated checks show counts in `MATRIX.md`.
- `round1-early-harness/` holds the first runs, on the first-published heads, with an earlier harness revision. Several of those checks were later corrected.
