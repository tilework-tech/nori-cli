# Using the TUI

This guide explains how to use the Nori terminal UI. The second half is a
reference: every slash command and every key. Terms follow
[GLOSSARY.md](../GLOSSARY.md) and the monorepo glossary
(`nori-monorepo: GLOSSARY.md`).

Related documents:

- Commands and flags (`nori`, `nori resume`, `nori attach`, `nori host`):
  [commands.md](commands.md)
- `[tui]`, `[tui.hotkeys]`, and every other setting:
  [configuration.md](configuration.md)
- Turns, the prompt queue, steering, goals, and delegation:
  [turns.md](turns.md)
- Approval modes and `/approvals`: [permissions.md](permissions.md)
- Transcripts, history, and logs on disk: [storage.md](storage.md)
- The host wire that `nori attach` speaks:
  `nori-monorepo: docs/specs/nori-cli/host-contract.md`

## Start the TUI

```bash
nori                       # default agent
nori --agent codex         # choose an agent
nori "summarize README"    # start with a prompt
nori -C ../other-repo      # work in another folder
```

The first launch shows the animated onigiri welcome above a short setup form;
one Enter accepts it:

- **Default agent** (required). Prefilled with the first agent Nori can see a
  sign-in for; each agent shows "signed in", "not signed in", or "sign-in not
  checked". Saved as `agent`.
- **Permissions**, prefilled with Agent. Ask saves `approval_policy =
  "untrusted"` and Approve all saves `"never"`; Agent saves nothing, so each
  folder's trust decides.
- **Usage stats**, prefilled with Share. Don't share sets `opt_out` in
  `$NORI_HOME/.nori-install.json`. Only counts are sent; never prompts, code,
  file paths or sessions.

When the intro plays (terminals about 40 rows and taller), the form fades in
top-down once the ball lands; in shorter terminals it shows at once. A key
pressed before the form is fully drawn shows it at once and is otherwise
ignored, so an early Enter never submits answers you have not seen.

`↑`/`↓` move, `←`/`→` or `Space` change an answer, `q` quits. Nori then asks
whether you trust the folder and saves the answer per project in
`config.toml`.

At startup the welcome card's title is Jiro's face, `〉_ ▐`, all in green. It
blinks while the agent session starts, then winks and types the wordmark
`〉nori CLI` and its version once the session is ready. You can type from the
first frame; nothing waits for the animation. The composer prompt is the same
`〉`. Set `[tui] animations = false` to show the wordmark at once. On a
terminal that draws ambiguous-width characters wide, the closed-eye frame can
shift by a cell.

## Who owns the sessions

Plain `nori` starts a **private host** for the run. The host lives inside the
`nori` process:

- It has no socket, no token, no lock, and no discovery file.
- The TUI is its only client. The TUI is always the controller.
- No other terminal or app can attach to its sessions.
- When you quit, the host stops. Every session ends, including a session that
  is in the middle of a turn.

`nori resume` and `nori cloud` work the same way.

To keep sessions running after the terminal closes, or to watch them from a
second terminal, run them on a background host instead. See
[Attach from a second terminal](#attach-from-a-second-terminal).

## Sessions

One TUI runs many sessions at once. Each session has its own agent. One
session is on screen. The others are **parked**: they keep running and keep
their own history.

- `/new` starts a session beside the current one. The current one is parked.
- `/agent` picks an agent and starts a session with it.
- `/resume` and `/fork` also start a session beside the current one.
- The number of sessions that hold an agent process is limited by
  `[tui] max_sessions` (default 8). At the limit, Nori refuses to start
  another and names the setting.

### Move between sessions

| Key | Action |
|---|---|
| `alt+,` / `alt+.` | Previous / next session. Wraps around and skips closed and done sessions. |
| `alt+down` | Next session that waits on you. Your draft stays with its session. |
| `alt+up` | From a delegated child, go to its parent. Does nothing elsewhere. |
| `alt+s`, or Left on an empty prompt, or `/sessions` | Open the session list. |

### The session list

Each row shows a status glyph, the title, the agent, the status, and context
use. A second line shows the latest reply or the pending request, the model,
and how long ago the session was active.

| Key | Action |
|---|---|
| Enter | Show the session. |
| Tab / Shift+Tab | Toggle the "waiting on you" filter. |
| Ctrl+D | Close the session. A working, waiting, or starting session needs a second press. On a closed or done row, Ctrl+D removes the row. |
| Ctrl+R | Resume a failed session. |
| Up/Down, j/k, Ctrl+P/Ctrl+N, PgUp/PgDn, Home/End | Move. |
| `/`, `f`, or Ctrl+F | Search. Esc ends the search. |
| Esc or Ctrl+C | Close the list. |

Status values: starting, working, waiting on you, ready, failed, closed, done.
A session that finished while it was off screen reads "ready · new". A parent
row shows the most urgent status of its family: waiting, then failed, working,
starting, ready, closed.

The title is the first one of: the agent's own title or prompt summary, the
first user message, the `/delegate` task, "new session".

### Close a session

- `/close` closes the session on screen. Another open session comes on screen.
- Closing the only open session is refused. Use `/quit` to leave Nori.
- Closing a session also closes its delegated children.

### Background sessions

A parked session keeps receiving output. When a top-level parked session needs
you or finishes, Nori sends a notification, `<title>: <message>`. Terminal and
OS notifications are settings in `/settings`.

## Resume a session

A resume reopens a conversation in a new session. Nori loads or resumes the
agent's own session when the agent supports it. Otherwise it starts a new
session with the transcript as context.

From the shell:

| Command | Behaviour |
|---|---|
| `nori resume` | Full-screen picker of sessions in this folder's project. |
| `nori resume --all` | Picker of sessions from every project. |
| `nori resume --last` | Resume the newest session in this folder without a picker. Starts fresh when there is none. |
| `nori resume <id>` | Resume that session, from any project. |

`nori resume` uses the agent that recorded the session. `--agent` with a
different agent is an error. Aliases of the same agent are accepted.

Picker keys: Enter resumes, Esc starts a fresh session, Ctrl+C exits,
Up/Down or j/k move, PgUp/PgDn page, Home/End jump, `/`, `f`, or Ctrl+F
searches.

From inside the TUI:

- `/resume` lists sessions. When the agent supports ACP `session/list` and
  either `session/load` or `session/resume`, the list comes from the agent and
  has a "Start a new session" row. Otherwise the list comes from local
  transcripts for this project and agent.
- `/resume-viewonly` prints an earlier transcript into the current history,
  between "Viewing Previous Session" and "End of Transcript". It starts no
  session.
- In the session list, Ctrl+R resumes a failed session. It keeps the same
  conversation ID, slot, parent, and title.

When you quit, Nori prints the command that resumes the session on screen:
`nori resume <id>`.

## Fork and rewind

Fork and rewind both start a new session and park the current one. They give
different results.

| | Fork | Rewind |
|---|---|---|
| What it does | ACP `session/fork` at the current point | Go back to an earlier user message |
| History the agent gets | The full history | A plain-text summary up to that message |
| Agent process | The same process | A new process |
| Needs | An agent that supports `session/fork` | A transcript on disk |

Use them like this:

- `/fork` opens a picker. Its first row, "Branch from current point", forks.
  It appears only when the agent supports fork. Nori prints
  "Session forked. To resume previous: nori resume <id>".
- The other rows of `/fork` are earlier user messages. Choosing one rewinds to
  it.
- Esc twice on an empty prompt also rewinds. The first Esc shows a hint. The
  second opens the transcript at the newest user message. Each further Esc
  steps to an older message. Enter confirms; any other key cancels. This works
  only when no turn is running, no popup is open, and the prompt is empty.

After a rewind, the prompt holds the chosen message so you can edit and send
it.

## Turns, the queue, and approvals

- Enter sends a prompt. While a turn you started is running, Enter **steers**
  the message into that turn and Tab **queues** it for the next turn. When the
  agent cannot accept a steer, Nori queues the message and says so. Rules:
  [turns.md](turns.md).
- Esc or Ctrl+C cancels the running turn.
- Up on an empty prompt focuses the queue. In the queue, j/k move, Enter or
  `e` takes the message back into the prompt, Delete or `x` removes it, and
  Esc leaves the queue.
- After a cancelled or failed turn, the queue pauses. Enter on an empty prompt
  resumes it.
- A permission request opens an approval overlay. See
  [Approval overlay](#approval-overlay) for keys and
  [permissions.md](permissions.md) for modes.

## Attach from a second terminal

`nori host` runs a background host. It owns its sessions, so they survive
when a terminal closes. `nori attach` opens the same TUI as plain `nori`,
joined to that host instead of owning one.

```bash
nori host                         # start the host (or report the running one)
nori host new --agent codex       # start a session and print its ID
nori attach                       # open the session list
nori attach 3f2a                  # open a session by an ID prefix
nori host ls                      # list sessions
nori host stop                    # stop the host and all its sessions
```

Rules:

- There is at most one background host per `NORI_HOME`.
- Detaching never stops the host or its sessions.
- `nori attach` starts on the alternate screen. `/screen` or Ctrl+L switches
  to the inline view, printing the history once, and back. Leaving from the
  alternate screen prints no transcript and no exit summary. It skips the
  welcome, trust, update, and worktree steps.
- Without a session ID, the session list opens, even when the host has no
  sessions. If it closes with nothing open, the screen says "No session
  selected. Use /new…".
- When a session on the host ends, the terminal stays: it shows the next
  session, or "Session ended. Use /new…".
- `nori attach --host <ws-url> --token-file <path>` attaches to a host that is
  not the local one. Both flags are required together.

### Controller and watcher

Each client of a host is a **controller** or a **watcher**.

- The first client that connects becomes the controller. Later clients are
  watchers.
- Only the controller can prompt, start or change sessions, and answer
  permission requests. A watcher sees every update.
- `alt+t` takes control. The previous controller becomes a watcher.
- When the controller disconnects, no client is promoted. Work keeps running.

One hint row above the footer shows this terminal's standing (`controlling`,
`watching · read-only`, `RECONNECTING`, or `DISCONNECTED`), the session's
folder, agent, and host status, `Approvals: <mode>`, and the keys that apply
(`ctrl+d detach`, `alt+t take control`).

### Attach keys and commands

| Key or command | Action |
|---|---|
| Enter | Send. During a turn, steer. |
| Tab | During a turn, queue. In `/new`, complete a folder name. |
| Ctrl+C | As the controller, cancel the running turn, also one another terminal started; the draft is kept. Once that cancel is pending, Ctrl+C is not resent: it keeps a typed draft and on an empty prompt detaches. A watcher or a reconnecting terminal never cancels, with Ctrl+C or Esc: during a turn it prints why and keeps the draft. On a stopped host, or when nothing runs, clear the draft; on an empty prompt, detach. |
| Ctrl+D, `/quit`, `/exit`, `/detach` | Detach. In the session list, Ctrl+D closes the selected session; in an empty list it detaches. |
| `alt+t` | Take control. |
| `alt+s`, Left on an empty prompt, `/sessions` | Open the session list. |
| `alt+down` | Next session that waits on you. |
| `alt+up` | Go to the parent session. |
| Ctrl+R (in the list) | Resume a failed session. |
| PgUp, Ctrl+T | Open the transcript (PgUp: in the full-screen view, a page above the screen); Esc or q closes it. |
| `/new [folder] [agent]` | Start a session. Without a folder, it uses the folder of the session on screen. With no session, a local host uses the terminal's folder and a `--host` host uses its own folder. A host that lists paths resolves the folder and completes it on Tab. The new session comes on screen once the host lists it. |
| `/agent`, `/agents` | Pick one of the agents the host lists, each checked on the host's machine: a `--host` terminal sees the host's agents, not its own. With no session on screen, the host starts a session with it. A session with no prompts yet changes agent in place and keeps its ID. A prompted session keeps running and the agent starts in a new session beside it, in the same folder. A row the host cannot start shows the host's reason, and Enter keeps the picker open. Saved sessions are not offered. |
| `/model`, `/config` | As the controller, change the model or ACP options the session on screen advertised. Only advertised values are offered (no custom model), and `/config` leaves out the agent (`/agent`) and permission mode (`/approvals`). The choice goes to the host, and the screen changes only when the host answers. Nothing is saved to `[default_models]`. |
| `/mention` | Insert `@` to mention a file on the host (see `@` file mention in [Composer features](#composer-features)). A watcher can use it too. |
| `/delegate <agent> <task>` | Delegate a task from the session on screen. |
| `/close` | Close the session on screen. |
| `/approvals ask\|agent\|approve-all` | Set the permission mode of the session on screen. `approve-all` asks first. An unknown mode keeps the typed command. |
| `/cancel` | Cancel the turn on screen. |

Esc does not edit an earlier prompt in `nori attach`. Other commands are
greyed with "not available in nori attach yet" or "not available on this
host yet"; a typed one is refused as `/<command>: <reason>` and stays in the
prompt. A command this terminal does not know, such as an agent's `/review`,
is sent to the agent as a prompt.

Permission and delegation requests open the same overlays as in plain `nori`
(see [Approval overlay](#approval-overlay)); only the controller can answer.

### When the connection drops

- The client reconnects every 750 ms. Requests that were in flight are not
  sent again.
- If the host restarted, the terminal shows "Host restarted; sessions were
  lost." and stays `DISCONNECTED`. Detach and run `nori attach` again.
- On a 401 or 403 from the local host, attach rereads the token once.

Host troubleshooting: [runbooks/debugging.md](runbooks/debugging.md#host).

## nori cloud

`nori cloud` is the TUI with Handroll as its agent. Handroll connects to a
conversation on the Nori Sessions broker. Differences from plain `nori`:

- It opens a session picker first. `nori cloud --onboard` resumes the
  onboarding session.
- There is no welcome, trust, or worktree step.
- Steering and Tab-to-queue are not available. Enter during a turn queues the
  message for the next turn.
- Commands marked "local only" below are refused.
- `/close` releases the cloud session.
- Quitting leaves the cloud session running.

Handroll must be on `PATH`, or named by `NORI_HANDROLL_BIN`. Handroll's own
contract lives in the Handroll repo.

## Terminal status

With `[tui] program_status = true`, Nori tells the terminal what the
foreground session is doing, using the
[Program Status Protocol](https://www.superlogical.com/rex/docs/build/program-status)
(OSC 7501, rev 0.4). Terminals that support it, such as kitty and iTerm2, can
show the state as a tab indicator or notification. Other terminals ignore it.
Nori does not probe for support.

| State | When |
|---|---|
| `blocked` | A permission request or a delegation waiting for approval is open. The oldest one is reported. |
| `working` | A prompt turn is running or cancelling, or the agent runs a turn of its own. |
| `done` | The last turn ended with any stop reason except cancelled. |
| `error` | The prompt failed, or the session ended with a lost connection, a spawn failure, or a timeout. |
| `idle` | Before the first turn and after a cancelled turn. |

- `done` and `error` stay until the next prompt starts. Typing does not
  change them. A restored transcript starts at idle.
- A blocked report says "Approval needed". Set
  `[tui] program_status_approval_titles = true` to send the real approval
  title instead. The title is made single-line and cut to 2048 bytes.
- Only the foreground session reports. Switching sessions reports the new one
  at once. `/screen` does not change the report.
- The tab title keeps naming the agent. Reports never set a title.
- `nori attach` reports the selected session the same way, in controller and
  watcher terminals.
- Nori clears the report on exit, on a `nori cloud` detach, on detach from
  `nori attach`, and on a panic.
- tmux 3.8 and earlier drop the sequence. Newer tmux forwards the active
  pane's report. Nori does not use DCS passthrough.
- A Ctrl+Z suspend keeps the last report until the TUI resumes. An attached
  terminal whose host stops keeps its last report until it exits.

## Leave the TUI

`/quit`, `/exit`, the bare words `exit` or `quit`, Ctrl+D on an empty prompt,
or Ctrl+C when nothing is running. Nori prints a summary of the session on
screen and the `nori resume <id>` command.

## Reference: slash commands

Type `/` to open the command popup. The popup lists commands in the order
below.

Columns:

- **During a turn:** whether the command runs while a turn you started is
  running. "beside" means it starts a new session and parks the busy one.
- **Scope:** "local only" commands are refused in `nori cloud`. "cloud" means
  the command has its cloud meaning only in `nori cloud`.

| Command | During a turn | Scope | What it does |
|---|---|---|---|
| `/agent` | beside | all | Pick an agent and start a session with it. Saves the choice as `agent`. In the picker, Shift+Tab turns ACP wire recording on or off. `/agents` is an alias; the menu lists `/agent` once. |
| `/model` | no | all | Pick the agent's model, or enter a custom one. Saves it in `[default_models]`. In `nori attach`, see [Attach keys and commands](#attach-keys-and-commands). |
| `/config` | no | all | Change the agent's own ACP session options. Only model choices are saved. In `nori attach`, see [Attach keys and commands](#attach-keys-and-commands). |
| `/approvals` | no | all | Choose what Nori can do without approval, for this session. See [permissions.md](permissions.md). |
| `/settings` | no | all | Open the Nori settings, including hotkeys and footer segments. |
| `/vim` | no | all | Choose Vim mode and what Enter does. |
| `/screen` | yes | all | Switch between the inline and the full-screen view. |
| `/goal [text\|pause\|resume\|clear\|edit]` | yes | all | Show, set, replace, pause, resume, clear, or edit the session goal. See [turns.md](turns.md). |
| `/new` | beside | all | Start a session. |
| `/sessions` | yes | all | Open the session list. |
| `/delegate <agent> <task>` | yes | all | Start a child session for a task. Bare `/delegate` shows a pending approval again. |
| `/resume` | beside | all | Resume an earlier session. |
| `/resume-viewonly` | no | all | Print an earlier transcript, read-only. |
| `/close` | no | cloud | Local: close the session on screen. Cloud: release the session. |
| `/init` | no | all | Ask the agent to write an `AGENTS.md`. Skipped when one exists. |
| `/compact` | no | all | Summarize the conversation to free context. |
| `/undo` | no | all | Ask the agent to undo a turn. |
| `/browse` | yes | local only | Open the file manager set in `/settings`. |
| `/diff` | yes | local only | Show `git diff`, including untracked files. |
| `/mention` | yes | all | Insert `@` to mention a file. |
| `/status` | yes | all | Show the session configuration and context use. |
| `/context` | yes | all | Show context use and the instruction files the agent loads. For a session on a host, both come from the host, so the files are the ones in the session's folder on that machine. Does not prompt the agent. |
| `/memory` | yes | all | Show the contents of the active instruction files. |
| `/first-prompt` | yes | all | Show the first prompt of the session. |
| `/mcp` | no | all | Manage MCP servers. Changes apply to new sessions. See [mcp.md](mcp.md). |
| `/login [agent]` | no | all | Sign in to the agent. |
| `/logout` | no | all | Show how to sign out of the agent. |
| `/api-key` | no | local only | Choose between the agent's own login and a saved API key. See [auth.md](auth.md). |
| `/quit`, `/exit` | yes | all | Leave Nori. Never disabled. |
| `/switch-skillset [name]` | no | local only | Switch skillsets. |
| `/fork` | no | all | Fork at the current point, or rewind to an earlier message. |
| `/browser` | no | local only | Unix only. Start a Chrome that the agent controls over CDP. |

Other popup entries:

- `/prompts:<name>`: a custom prompt from `$NORI_HOME/commands/`. A prompt is a
  Markdown template (`.md`) or a script (`.sh`, `.py`, `.js`). Templates fill
  `$1` to `$9`, `$ARGUMENTS`, and named `$NAME` placeholders. For a session on
  a host, the list comes from the host's own `$NORI_HOME/commands/`, so every
  terminal on that host, attached ones included, offers the same prompts.
  Templates are filled in by the terminal. Scripts run on the host, in the
  session's folder, and only the controller may run one.
- `/<prefix>:<name>`: a command the agent advertises. Nori sends it without the
  prefix.
- An agent command whose name matches a built-in is not shown.
- An unknown `/name` prints "Unrecognized command" and keeps the draft.

## Reference: keys

### Configurable keys

Set these in `[tui.hotkeys]`, or in `/settings` > Hotkeys. Format:
`[mod+[mod+]]key`, with at most two of `ctrl`, `alt`, `shift`. Keys include
named keys and `f1` to `f12`. `"none"` removes the binding. In the Hotkeys
view, Enter captures the next key, `r` resets, and choosing a key that is
already bound swaps the two.

| Action (`[tui.hotkeys]` key) | Default |
|---|---|
| `open_transcript` | `ctrl+t` (`ctrl+o` always works too) |
| `open_editor` | `ctrl+g` |
| `move_backward_char` | `ctrl+b` |
| `move_forward_char` | `ctrl+f` |
| `move_beginning_of_line` | `ctrl+a` |
| `move_end_of_line` | `ctrl+e` |
| `move_backward_word` | `alt+b` |
| `move_forward_word` | `alt+f` |
| `delete_backward_char` | `ctrl+h` |
| `delete_forward_char` | `ctrl+d` |
| `delete_backward_word` | `ctrl+w` |
| `kill_to_end_of_line` | `ctrl+k` |
| `kill_to_beginning_of_line` | `ctrl+u` |
| `yank` | `ctrl+y` |
| `history_search` | `ctrl+r` |
| `toggle_plan_drawer` | `alt+p` |
| `previous_session` | `alt+,` |
| `next_session` | `alt+.` |
| `next_waiting_session` | `alt+down` |
| `open_session_list` | `alt+s` |
| `parent_session` | `alt+up` |
| `toggle_screen` | `ctrl+l` |

`nori attach` uses the same keys.

### Fixed keys

| Key | Action |
|---|---|
| Enter | Send. During a turn you started, steer. |
| Tab | During a turn you started, queue the draft. Otherwise complete. |
| Shift+Enter, Alt+Enter, Ctrl+J | New line. |
| Esc | During a turn, cancel it. On an empty prompt, start a rewind (press twice). |
| Ctrl+C | Close the open view; else clear the prompt; else cancel the turn; else quit. |
| Ctrl+D | On an empty prompt, quit. Otherwise delete forward. |
| Ctrl+V | Paste an image from the clipboard. |
| Shift+Tab | Cycle the agent's mode option. Not saved. |
| Up on an empty prompt | Focus the queue. With no queue, recall history. |
| Up/Down, Ctrl+P/Ctrl+N | Recall prompt history. |
| Enter on an empty prompt | Resume a paused queue. |
| Left on an empty prompt | Open the session list. |
| `?` on an empty prompt | Show the shortcut overlay. |
| `!` at the start | Shell mode: run the line as a shell command on the session's host, in the session's folder. The command and its output show in the transcript, for every terminal following the session. Only a local controller can run one. A watcher, or a terminal on another machine, gets the line back in its prompt. |
| `@` | Mention a file (fuzzy search). |
| `$` | Pick an agent skill or command. |
| Ctrl+Z | Suspend Nori. |

A dim row under the prompt shows the keys that work now: `/ commands · ! shell
· @ files · ? help` on an empty prompt (`!` and `@` only where they work, and
`Enter resume queue` first when the queue is paused), `Enter send ·
Shift+Enter newline` once text is typed, `Enter steer · Tab queue` or
`Enter queue` during a turn you started, and `Enter run` for a slash command
or `!` line. Turn it off with the `composer_hints` footer segment
([configuration](configuration.md#tuifooter_segments)).

In the transcript overlay, `q`, Ctrl+C, Ctrl+T, or Ctrl+O closes it.

In every picker: Up/Down or j/k move, PgUp/PgDn and Home/End page, and
typing filters only during a search. Lists with search (`/settings`,
`/sessions`, `/resume`, `/model`, skillsets) start one with `/`, `f`, or Ctrl+F,
show `/ search` in their footer, and Esc ends the search. Short lists such as
`/agent` and `/config` have no search.

### Approval overlay

The overlay asks "Would you like to allow ...?" and lists the agent's options.

| Key | Answer |
|---|---|
| `y` | Allow once |
| `a` | Allow always |
| `n` or Esc | Reject once (Esc falls back to reject always, then cancel, when the agent offers no reject-once option) |
| `d` | Reject always |
| Enter, digits | Choose from the list |
| Ctrl+A | Show the request full screen |
| Ctrl+C | Cancel this request and drop the other waiting requests |

Keys are ignored for 400 ms after a request appears, so a key typed for the
prompt does not answer it. When several requests wait, the newest shows first.

### Composer features

- **`@` file mention:** fuzzy search in the session's folder, on the host
  that runs it, following the session on screen. Without a host session, Nori
  searches the launch folder; in `nori attach`, a host that lists paths
  searches its own folder, and other hosts are not searched. Tab or Enter
  inserts the path, quoted if it contains spaces. An image file is attached;
  in `nori attach`, its host path is inserted as text instead.
- **History search:** Ctrl+R searches up to 500 earlier prompts from
  `$NORI_HOME/history.jsonl`. For a session on a host, that is the host's
  file, so Up and Ctrl+R also find prompts sent from another terminal on the
  same host. Up sees those after the next turn ends.
- **`!` shell commands:** `!cmd` runs `$SHELL -lc cmd` on the host that
  runs the session, in the session's folder, as the host's user, with no
  sandbox and no approval (see [permissions](permissions.md)). Only a
  *local* client may run one: plain `nori` (its in-process host), or
  `nori attach` that found the host through local discovery and so sends the
  host's private local token (see [storage](storage.md#host-files)). Every
  other client is remote: `nori attach --host URL --token-file F`, and every
  stdio client (`nori host --stdio`, `nori exec --acp`, the join relay),
  which may be relayed from anywhere. A remote `!cmd` runs on neither
  machine; the TUI shows the gray hint "Remote shell commands are not yet
  supported." and puts `!cmd` back in the prompt.
- **`_nori/session/shell`** {sessionId, command} is the host method. It
  replies once the command starts. A watcher gets `-32016`; a remote
  controller then gets `-32018` with the message above, before the session
  is looked up; an empty command gets `-32602`, and an unknown session
  `-32002`. The command, its output and its exit code arrive as that
  session's `user_shell` events, so every terminal following the session
  shows a `Ran` cell, also on replay. A failed command's bullet is red.
  Output is shown once the command finishes. `initialize` advertises
  `capabilities.nori_sessions.shell` (host contract 1.9.0) per connection:
  `true` only for a local client. Without a host session the command runs in
  process, the same way. When a joined host does not offer `shell`, `nori
  attach` does not send `!cmd`: with `--host` it shows the gray hint above,
  and on a local host too old for `!` it says "not available on this host
  yet". Either way the line goes back to the prompt and never reaches the
  agent.
- **Paste:** a paste over 1000 characters shows as
  `[Pasted Content N chars]` and expands on send. A pasted image path becomes
  an attachment.
- **External editor:** Ctrl+G opens `$VISUAL`, then `$EDITOR`, then `vi`
  (`notepad` on Windows). Saving with exit code 0 replaces the draft.
- **Vim mode:** `/vim` chooses Off, Submit in NORMAL, Submit in INSERT, or
  Always Submit. Normal mode has motions, edits, and the `d`, `y`, `c`
  operators with text objects. Counts are not supported.

## Where `main` differs

- `/close` on the only open session closes it and opens the saved-session
  picker. Only Ctrl+D in the session list refuses to close the last open
  session.
- The `/fork` picker is titled "Fork conversation" and its fork row reads
  "Branch from current point"; it does not offer separate "Fork" and "Rewind"
  entries.
- `/goal pause` and the goal status "paused" use "pause", which is reserved
  for the prompt queue.
- A closed session does not stay resumable: Ctrl+D removes a closed row, and
  Ctrl+R resumes only failed sessions.
