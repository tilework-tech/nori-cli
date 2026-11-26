# Permissions

This spec states who answers an agent's permission requests, how each
permission mode maps to the approval policy and to each adapter's own mode,
how unattended runs behave, and what Nori does and does not sandbox. It
refines the "Permissions" section of the nori-monorepo product spec
(`docs/specs/nori-cli/product-spec.md`). Terms follow
[GLOSSARY.md](../GLOSSARY.md) and the nori-monorepo `GLOSSARY.md`.

## Principles

- **The agent runs its own tools.** Nori stops a tool call only when the
  agent asks about it with `session/request_permission`. To make an agent ask
  more, Nori sets the agent's own mode.
- **Nori decides who answers.** The permission mode says whether Nori answers
  a request itself or relays it to the user.
- **Unattended runs never hang** on a permission request.
- **Bypass is explicit.** Only `--yolo`, `approve-all`, or an explicit
  setting turns approvals off.

## Permission modes

Every session has one permission mode.

| Mode | Name in UIs | Approval policy | Who answers requests |
|---|---|---|---|
| `ask` | Ask | `untrusted` | The user. The adapter asks before every edit and command. |
| `agent` | Agent | `on-request` | The user. The adapter decides what to ask about. |
| `approve-all` | Approve all | `never` | Nori approves every request itself. |

The approval policy `on-failure` reads as `agent`; for ACP agents it asks
exactly as `on-request` does.

### The adapter's mode behind each permission mode

| Agent | `ask` | `agent` and `approve-all` |
|---|---|---|
| `claude-code` | adapter mode `default` | adapter mode `acceptEdits` |
| `codex` | adapter mode `read-only` | adapter mode `workspace-write` |
| `gemini`, `grok-build`, `opencode`, `cursor`, `hermes`, `pi`, custom agents | not offered | the agent keeps the mode it chose |

- Nori offers `ask` only when the agent's `mode` option lists the value in
  the table.
- Nori sets the adapter's mode with `session/set_config_option` on the
  agent's `mode` option.

### Answering a request

- Under `approve-all`, Nori picks the request's first `allow_once` or
  `allow_always` option. A request with no allow option is answered as
  cancelled.
- Under `ask` and `agent`, Nori relays the request to the client. On
  `nori host` it goes to the controller only, and the first valid answer
  wins. The TUI also sends an "awaiting approval" notification.
- Cancelling a turn answers its pending requests as cancelled.
- A request that arrives when no turn runs is answered as cancelled.

## Setting the mode

### Default

| Source | Result |
|---|---|
| `approval_policy` in `config.toml` or `-c approval_policy=...` | That policy's mode |
| Not set, the folder's project is marked untrusted | `untrusted` (`ask`) |
| Not set, any other folder | `on-request` (`agent`) |
| `--yolo` | `never` (`approve-all`) |

### `nori host`

- `[host] default_mode` (`ask`, `agent`, or `approve-all`) sets the mode of
  every session a client starts with ACP `session/new`. When it is not set,
  each session takes its folder's `approval_policy`.
- A client changes the mode with `session/set_mode` or with
  `session/set_config_option` on `mode` (nori-monorepo
  `docs/specs/nori-cli/host-contract.md`). Nori sets the adapter's mode
  first, then the session's. `ask` fails when the adapter cannot ask before
  every edit and command. An adapter that refuses its mode change leaves the
  session unchanged.
- When a session starts or restarts in `ask` and its adapter cannot ask
  every time, the session continues in `agent`, and attached clients get the
  new mode.
- Every change reaches attached clients as `current_mode_update` and
  `config_option_update`.
- The host remembers each session's mode and applies it again when the
  session restarts.

### Child sessions

A child session starts in its parent's permission mode
([turns.md](turns.md#delegation)).

### `/approvals`

`/approvals` in the TUI offers the three permission modes, Ask, Agent, and
Approve all, with the names and meanings in the table above. Choosing one
changes the current session's mode as a `session/set_mode` does.

## Unattended runs: `nori exec`

`nori exec "<prompt>"` (and `nori -p "<prompt>"`) runs one prompt headlessly.

- It runs on the running background host when that host speaks a compatible
  host contract, grants it control, and the run carries no configuration
  overrides (`-c` or `--add-dir`). Otherwise it starts a private in-process
  host.
- It starts one session, sets its mode, sends the prompt, prints the agent's
  text, and closes the session.

| Flags | Mode | Permission requests |
|---|---|---|
| none | `agent` | Rejected: Nori picks the `reject_once` or `reject_always` option, or answers cancelled when there is none |
| `--dangerously-bypass-approvals-and-sandbox` | `approve-all` | Approved by Nori |

- A rejected request does not stop the turn. After the turn, `nori exec`
  prints the output and exits with the error "permission request was
  rejected during unattended execution".
- Ctrl+C or `SIGTERM` cancels the turn, closes the session, and exits with an
  error.
- `nori exec --acp` serves the headless host over stdio, as
  `nori host --stdio` does. It keeps the configured approval policy; with the
  bypass flag it uses `never`.

## Bypassing approvals: `--yolo`

`--dangerously-bypass-approvals-and-sandbox` sets the approval policy to
`never` and `sandbox_mode` to `danger-full-access` for the run.

| Command | Flag form |
|---|---|
| `nori`, `nori resume`, `nori cloud` | `--dangerously-bypass-approvals-and-sandbox` or `--yolo` |
| `nori host` | `nori --yolo host ...`; applies to the sessions of the host it starts |
| `nori attach` | Accepted, but has no effect on the host's sessions; the host applies its own configuration |
| `nori exec` | `--dangerously-bypass-approvals-and-sandbox` (no `--yolo` alias), or `nori --yolo exec ...` |
| `nori -p` | `nori -p --yolo ...` |

No setting turns bypass on by default.

## Sandboxing

### What Nori does not sandbox

- **The agent's tools.** Nori does not confine the agent process or its
  tools on the user's machine. Any confinement comes from the agent itself,
  for example Codex's `read-only` mode behind `ask`.
- **User shell commands** (`!` in the TUI). They run directly, as the
  user the host runs as, on the host's machine, in the session's folder,
  with no approval. Only the host's controller can run one
  (`_nori/session/shell`), and only from the host's own machine: the client
  must present the host's private `local-token`. Watchers get `-32016`;
  remote controllers get `-32018` and nothing runs. A client that can read
  `local-token` can already run commands as the host's user, so the token
  grants nothing new. The host listens only on `127.0.0.1`, so the peer
  address cannot tell a tunnel or relay from a local client; the token can.
  See [tui](tui.md#composer-features).
- On a session machine, the machine is the isolation boundary
  (nori-sessions).

### Sandbox settings: not enforced yet

These settings are parsed and shown, but nothing on the ACP path enforces
them yet. Per-harness sandboxing, which will put them to use, is being built
([monorepo #263](https://github.com/tilework-tech/nori-monorepo/pull/263)).

| Setting | Values | Default |
|---|---|---|
| `sandbox_mode` | `read-only`, `workspace-write`, `danger-full-access` | `workspace-write` |
| `[sandbox_workspace_write] writable_roots` | list of paths | empty |
| `[sandbox_workspace_write] network_access` | bool | `false` |
| `[sandbox_workspace_write] exclude_tmpdir_env_var` | bool | `false` |
| `[sandbox_workspace_write] exclude_slash_tmp` | bool | `false` |
| `--add-dir <DIR>` | adds a writable root | none |
| `--yolo` | sets `danger-full-access` | off |
| `[features] enable_experimental_windows_sandbox` | bool | off |

All keys and their precedence: `docs/configuration.md`.

### What Nori does sandbox

- **Side sessions on a managed host.** With `[host] side_sessions` on, a
  side session runs read-only under Bubblewrap when the Linux machine
  supports it. `[host] side_confinement = "required"` refuses a side session
  the machine cannot confine; the default `"preferred"` starts it unconfined
  and reports that. The wire is in nori-monorepo
  `docs/specs/nori-cli/host-contract.md`.

### `nori sandbox`

`nori sandbox` (alias `nori debug`) runs one command under a platform
sandbox. It exists to test the platform sandboxes; agents never run under
it.

| Subcommand | Alias | Platform | Sandbox |
|---|---|---|---|
| `nori sandbox macos [--full-auto] [--log-denials] -- <cmd>` | `seatbelt` | macOS | Seatbelt |
| `nori sandbox linux [--full-auto] -- <cmd>` | `landlock` | Linux | Landlock and seccomp |
| `nori sandbox windows [--full-auto] -- <cmd>` | none | Windows | Restricted token |

- Without `--full-auto` the command runs `read-only`. With `--full-auto` it
  runs `workspace-write`: it can write the folder and `TMPDIR`, with no
  network.
- `[sandbox_workspace_write]` and `shell_environment_policy` from the
  configuration apply.
- `--log-denials` (macOS) prints the sandbox denials after the command
  exits.

## Where `main` differs

- Choosing Approve all in `/approvals` opens a full-access confirmation.
  In plain `nori`, the `hide_full_access_warning` notice hides it.
