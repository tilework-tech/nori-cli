# Spec gaps

Every place where the code on `main` does not match the CLI specs. Each item is
one work item, sized for one PR. When an item lands, delete it from this file
and from the "Where `main` differs" list of each doc it names.

Specs referred to:

- **Host contract:** nori-monorepo `docs/specs/nori-cli/host-contract.md`
- **Registry:** nori-monorepo `docs/acp-extensions.md`
- **Product spec:** nori-monorepo `docs/specs/nori-cli/product-spec.md`
- CLI docs in this repo: `docs/*.md`, `nori-rs/*/docs.md`

Paths are relative to `nori-rs/` unless they start with `.github/`, `nori-cli/`,
or `justfile`. Size: **S** under a day, **M** a few days, **L** a week or more.

Items marked **Decision needed** cannot start until the owner answers the
question in the item.

## Host wire

### G01. Move Nori `initialize` fields into `_meta` (M)

- **Rule:** Nori additions on `initialize` ride in `_meta`:
  `_meta["nori/observe"]` on the request, `agentCapabilities._meta["nori/host"]`
  on the result. The host accepts and sends both forms for one minor version.
  (Host contract: `initialize`; Registry: `nori/observe`, `nori/host`.)
- **Today:** request root `params.observe` and
  `params.capabilities.nori_sessions.approval_modes`
  (`acp-host/src/remote/host_wire.rs::serve_frames`; a lifecycle connection
  always observes); result root fields `capabilities.nori_sessions`
  (including `find_paths`), `control`, `server_info`
  (`harness/src/managed_remote.rs`, `host-runtime/src/host/service.rs`).
  `agentCapabilities._meta["nori/preparedAgent"]` already uses `_meta`
  (`host-runtime/src/host/acp/handshake.rs`).
  Readers: `tui/src/host_client/mod.rs` (`HostFeatures`), `cli/src/host/mod.rs` (`session_cwd`
  check), and nori-sessions `native/src/main/host/probe.ts`.
- **Done when:** the host sends both forms and reads both; every cli reader
  uses the `_meta` form; `HOST_CONTRACT` gets a minor bump; schema
  regenerated.
- **Coordinate:** nori-sessions Native moves its probe in the same window.

### G02. Stop using `-32000` for generic errors (S)

- **Rule:** `-32000` means authentication required and is never sent for
  anything else. A method outside the managed set gets `-32601`. Other
  failures use the agent's own code, else `-32603`. (Host contract: Errors.)
- **Today:** "unsupported managed host operation"
  (`host-runtime/src/host/service.rs`) and agent start and admission failures
  (`harness/src/managed_remote.rs`) return `-32000`.
- **Done when:** no host path sends `-32000`; tests assert the new codes.
- **Coordinate:** check the broker (`managed-host.ts`) does not match on
  `-32000`.

### G03. Remove `_nori/session/{cancel,close,resume}` (S)

- **Rule:** one method per action; clients use ACP `session/cancel`,
  `session/close`, `session/resume`. Every listed row carries its `cwd`.
  (Host contract: The ACP face; Registry: Retired.)
- **Today:** the host serves all three (`host-runtime/src/host/service.rs`,
  `lifecycle`). The TUI host client calls `_nori/session/resume` when a row has
  no `cwd` (`tui/src/host_client/mod.rs`).
- **Done when:** the three methods return `-32601`; every row from
  `session/list` and `_nori/session/list` has `cwd`; the TUI uses ACP
  `session/resume` only.

### G04. Move `nori attach` and the TUI host client to the ACP face (L)

- **Rule:** the ACP face is the contract. `_nori/*` exists only for
  follow-ups, delegation, subscribing to every session, control, and stop.
  (Host contract: The ACP face, Nori additions.)
- **Today:** `tui/src/host_client/mod.rs` uses
  `_nori/session/{list,new,prompt,respond}`; `nori host ls` and `nori host new`
  (`cli/src/host/mod.rs`) use `_nori/session/{list,new}`. The host serves them
  in `harness/src/managed_remote.rs` and `host-runtime/src/host/service.rs`.
  The TUI host client already uses ACP `session/new`, `session/list`,
  `session/set_mode`, `session/close`, and `session/resume` (with a `cwd`).
  It also calls `_nori/host/prepare_agent`, `_nori/host/discard_agent`,
  `_nori/session/find_paths`, and `_nori/session/queue/{resume,take,discard}`,
  which the rule's list does not name yet.
- **Done when:** no cli client calls those four methods; the host still serves
  them, marked deprecated in the registry, until the next major.
- **Depends on:** G03.

### G05. Remove the host's "selected session" (M)

- **Rule:** every session-scoped call names its session. The host has no
  default session. (Host contract: The ACP face.)
- **Today:** each new session becomes the selected session, and `_nori/*`
  calls without `session_id` target it (`harness/src/managed_remote.rs`,
  `target`; `host-runtime/src/host/service.rs`;
  `host-runtime/src/host/acp/session.rs::start_and_attach`, which picker
  activations also use). `_nori/session/list`
  returns `selected_session_id`.
- **Done when:** `session_id` is required on every session-scoped `_nori/*`
  call (`-32602` without it); `selected_session_id` is gone.
- **Depends on:** G04 (the TUI must send IDs everywhere first).

### G06. One host per session (M)

- **Rule:** a host refuses to load a session that another host on the same
  `NORI_HOME` is running, including private hosts. (Host contract: One writer
  per session.)
- **Today:** a private stdio host and the background host each see the other's
  live sessions as stored and can both load one
  (`host-runtime/src/host/transcripts.rs::stored_sessions`,
  `host-runtime/src/host/acp/attach.rs::load_session`).
- **Done when:** a transcript carries an owner lock (for example a lock file
  or owner pid next to it); `session/load` and `session/resume` on a session
  owned by a live other host fail with a clear error; a stale lock from a dead
  process is taken over.

### G07. Rename `nori.dev/` keys to `nori/` (M)

- **Rule:** one prefix, `nori/`. The host accepts both prefixes for one minor
  version and sends `nori/` once consumers read both. (Host contract:
  Versioning; Registry: managed-host keys.)
- **Today:** `nori.dev/promptEchoId` (`nori-protocol/src/lib.rs`),
  `nori.dev/followUpSender` (`harness/src/follow_up.rs`),
  `nori.dev/managedHost`, `nori.dev/durableRecovery`, `nori.dev/followUp`
  (`host-runtime/src/host/service.rs`), `nori.dev/sideSession`
  (`acp-host/src/remote/mod.rs`), `nori.dev/sideConfinement`
  (`harness/src/managed_acp/side_worker.rs`), `nori.dev/readOnlyFork`
  (`harness/src/managed_acp/native.rs`).
- **Done when:** cli reads both prefixes everywhere and writes both; a
  follow-up PR drops `nori.dev/` writes after the broker reads `nori/`.
- **Coordinate:** nori-sessions broker (`managed-host.ts`, `acp/client.ts`)
  must read both before cli stops writing `nori.dev/`.

### G08. Remove unused managed replay surface (S)

- **Rule:** the managed wire has no `_nori/host/replay`, no
  `_nori/host/command`, and no `/managed` `attachFrom` replay. (Host contract:
  Managed wire; Registry: Retired.)
- **Today:** served by `host-runtime/src/host/recovery_control.rs` and
  `acp-host/src/remote/standard_wire.rs`; only tests call them.
- **Done when:** code and tests are removed; schema regenerated.

### G09. Honour client `mcpServers` on `session/load` and `session/resume` (S)

- **Rule:** client-sent `mcpServers` merge over host config for that session;
  the client entry wins on a name clash; the host remembers them. (Host
  contract: The ACP face.)
- **Today:** only `session/new` and picker activation merge them
  (`host-runtime/src/host/acp/session.rs::start_and_attach`,
  `host-runtime/src/host/acp/picker.rs::activate_selection`). Load and resume apply
  only the remembered set (`host-runtime/src/host/acp/attach.rs`,
  `host-runtime/src/host/acp/config.rs::restore_mcp_servers`).
- **Done when:** load and resume merge the sent list over the remembered one
  and store the result.

### G10. Advertise HTTP MCP support (S)

- **Rule:** `initialize` capabilities match what the host accepts.
- **Today:** `mcpCapabilities.http` is `false`
  (`host-runtime/src/host/acp/handshake.rs`), yet `session/new` accepts HTTP
  servers.
- **Done when:** `http` is `true` (or HTTP is refused; pick the one the agents
  support), with a test.

### G11. Guard client MCP servers on `nori host` (S)

- **Rule:** the `nori-client` server name is reserved for Nori. A client's own
  `Authorization` header is not overwritten. (`docs/mcp.md`.)
- **Today:** a client can send a server named `nori-client` on `session/new`
  or picker activation, so the agent gets two; an HTTP server with its own `Authorization` header still gets a stored
  OAuth header added.
- **Done when:** a client `nori-client` entry gets `-32602`; a client header
  wins over a stored token.

## TUI and product

### G12. Closed sessions stay listed and resumable (M)

- **Rule:** close stops the agent; the session stays listed and can be
  resumed. (Product spec: Next; Glossary: Close; `docs/tui.md`.)
- **Today:** Ctrl+D removes a closed row, and Ctrl+R resumes only failed
  sessions (`tui/src/bottom_pane/session_list_view.rs`,
  `tui/src/app/session_resume.rs`).
- **Done when:** closed rows stay until deleted; Ctrl+R resumes closed and
  failed sessions; PTY test covers it.

### G13. `/approvals` shows the three permission modes (M)

- **Rule:** `/approvals` offers `ask`, `agent`, `approve-all`, and applies the
  choice the same way as `session/set_mode` (adapter mode and remembered host
  mode). (`docs/permissions.md`, `docs/tui.md`; decision c06.)
- **Today:** the picker and footer show the three modes, and a host session
  changes through `session/set_mode` (`tui/src/chatwidget/approvals.rs`).
  Approve all still opens the full-access confirmation hidden by
  `hide_full_access_warning`, and `common/src/approval_presets.rs` remains for
  the Windows sandbox prompt.
- **Done when:** the full-access warning is gone and nothing in the TUI uses
  `common/src/approval_presets.rs`; snapshots updated.

### G30. Closing the last open session (S) — Decision needed

- **Rule:** closing the only open session is refused. (`docs/tui.md`.)
- **Today:** `/close` on the last open session closes it and opens the
  saved-session picker (`tui/src/app/session_lifecycle.rs::on_session_closed`).
  Only Ctrl+D in the session list refuses
  (`tui/src/app/session_lifecycle.rs::close_session`).
- **Decision needed:** keep the picker behaviour and change the spec, or
  refuse `/close` too.
- **Done when:** `/close` and Ctrl+D agree with the spec; PTY test covers it.

### G14. Separate fork from rewind (S)

- **Rule:** "fork" means ACP fork only; going back to an earlier message is
  "rewind". (`docs/turns.md`, `docs/tui.md`; decision c03.)
- **Today:** one picker titled "Fork conversation" offers "Branch from current
  point" and rewind (`tui/src/nori/fork_picker.rs`); `/fork` is described as
  "rewind conversation to a previous message"; rewind code is named
  "backtrack" (`tui/src/app_backtrack.rs`).
- **Done when:** the UI and the slash-command list name fork and rewind
  separately; internal names follow; snapshots updated.

### G15. Stop using "pause" for goals (S) — Decision needed

- **Rule:** "queue" and "pause" are reserved for the prompt queue in
  user-facing text. (Decision c04.)
- **Today:** `/goal pause` and the goal status "paused"
  (`harness/src/backend/goal_ext.rs`, `tui/src/chatwidget/goal.rs`).
- **Decision needed:** the replacement word (for example "suspend").
- **Done when:** command, status text, and help use the new word; the old
  command stays as a hidden alias for one release.

### G16. Steering gate by minimum adapter version (S) — Decision needed

- **Rule:** steering needs the adapter's steering capability plus a minimum
  version per adapter. (`docs/turns.md`, `docs/agents.md`; decision c05.)
- **Today:** `harness/src/follow_up.rs::supports_guarded_steering` allows an
  exact list: `claude-agent-acp` 0.102.0, 0.108.1, 0.109.0; `codex-acp` 2.7.0,
  2.8.0. Newer versions fall back to the queue.
- **Decision needed:** confirm the minimums. The docs assume 0.102.0 and
  2.7.0, the lowest versions on today's list.
- **Done when:** the check is a semver comparison against a per-adapter
  minimum, with tests at, below, and above the minimum.

### G17. Rename the `nori-harness` crate (M) — Decision needed

- **Rule:** "harness" means the upstream coding-agent project; the session
  runtime crate gets another name. (Glossary; decision c01.)
- **Today:** `harness/` is the crate `nori-harness`.
- **Decision needed:** the new name (for example `nori-runtime`).
- **Done when:** folder, crate, imports, CI, and docs use the new name; the
  "Where `main` differs" lines and `cli/tests/nori_harness_crate.rs` (which
  only checks `find_nori_home`) are updated.

### G18. Move Nori files out of the Codex home (M)

- **Rule:** Nori owns its files under `NORI_HOME`. Reading the Codex agent's
  own `~/.codex` for that agent is allowed. (`docs/mcp.md`,
  `docs/storage.md`; decision c08.)
- **Today:**
  - MCP OAuth fallback file `$CODEX_HOME/.credentials.json` or
    `~/.codex/.credentials.json` (`rmcp-client/src/oauth.rs::fallback_file_path`,
    `rmcp-client/src/find_codex_home.rs`). No migration.
  - The TUI session header lists `~/.codex/AGENTS.md` for every agent
    (`tui/src/nori/session_header/mod.rs`).
  - The Windows sandbox state folder uses `find_codex_home()`
    (`sandbox/src/exec.rs::exec_windows_sandbox`). The crate has no `config`
    module, so this path may not build.
  - The UI event log reads `CODEX_TUI_RECORD_SESSION` and
    `CODEX_TUI_SESSION_LOG_PATH`; the PTY harness sets `CODEX_HOME`
    (`tui-pty-e2e/src/lib.rs`).
  - The inherited `codex-core` config loader reads `$CODEX_HOME/config.toml`
    (`core/src/config/mod.rs::Config::load_with_cli_overrides`,
    `core/src/config/edit.rs`, `core/src/config_loader/mod.rs`); nothing in
    the binary calls it.
- **Done when:** the OAuth fallback lives in `NORI_HOME` with a one-time
  migration from the old file; the header lists `~/.codex/AGENTS.md` only for
  the Codex agent; the Windows path uses `NORI_HOME`; env vars are renamed to
  `NORI_*` (old names accepted for one release); the dead loader is removed.
- Can split into two PRs: the OAuth file with migration, then the rest.

### G19. Warn on MCP settings that do nothing (S)

- **Rule:** MCP settings that have no effect get a warning when config loads.
  (`docs/mcp.md`, `docs/configuration.md`; decision c09.)
- **Today:** `startup_timeout_sec`, `startup_timeout_ms`, `tool_timeout_sec`,
  `enabled_tools`, `disabled_tools` parse into `McpServerConfig` with no
  warning; unknown keys under `[mcp_servers.<name>]` are ignored silently
  (`nori-config/src/`).
- **Done when:** each of these logs one warning naming the server and key.

## Release

### G20. Snapshot tag and binaries from one commit (S)

- **Rule:** a tag and every asset of its release come from the same commit.
  (`docs/release-artifacts.md`.)
- **Today:** each job in `.github/workflows/nori-release.yml` checks out
  `main` again, and `create-next-tag` tags whatever `main` points to when it
  runs.
- **Done when:** the first job records the SHA and every later job and the tag
  use it.

### G21. Release archives keep the execute bit (S)

- **Rule:** `nori/nori` in each archive is executable. (`docs/release-artifacts.md`.)
- **Today:** workflow artifacts drop file modes; in 0.30.0 `nori/nori` is
  `-rw-r--r--`. Only the npm staging step restores the mode.
- **Done when:** the archive step sets `0755` before packing; a check in the
  workflow lists the mode.

### G22. Releases cannot change once published (S) — Needs repo admin

- **Rule:** tags and assets never change once published. (`docs/release-artifacts.md`;
  decision c10.)
- **Today:** the repo has no tag ruleset; a tag, release, or asset can be
  deleted or replaced by hand.
- **Done when:** a tag ruleset blocks deleting and moving release tags, and
  releases are immutable. This is a GitHub settings change: a repo admin must
  apply it.

### G23. npm dist-tags match the channels (S)

- **Rule:** `@next` carries snapshots; `latest` carries stable releases.
  (`docs/release-artifacts.md`.)
- **Today:** any version with a `-` suffix, alphas included, goes to `next`; a
  stable release also moves `next` to itself in a step that may fail and uses
  `NPM_TOKEN` instead of OIDC (`.github/workflows/nori-release.yml`).
- **Done when:** the owner's intended mapping for alphas is applied (confirm
  first), and every publish step uses OIDC.

### G24. npm package lists only shipped platforms (S)

- **Rule:** the package installs only where a binary ships. (`README.md`,
  `docs/release-artifacts.md`.)
- **Today:** `nori-cli/package.json` lists `android` in `os`, and
  `nori-cli/bin/platform.js` maps Android and Windows triples; no Android or
  Windows binary ships. An Android install succeeds, then fails to start.
- **Done when:** `os` and the platform map match the release targets.

### G25. Snapshot wording (S)

- **Rule:** `@next` snapshots are a supported channel. (Decision c10.)
- **Today:** the `nori-release.yml` header and `create_nori_release --help`
  call snapshots "for testing" and "for internal testing".
- **Done when:** both describe snapshots as the `@next` channel.

## Build, test, and logging

### G26. Fix `just test` targets (S)

- **Today:** `just test acp` runs `cargo test -p nori-acp`; the crate is
  `nori-acp-host` (`justfile`). `just test core` targets the inherited
  `codex-core`.
- **Done when:** every `just test` target names a real crate that the
  product uses.

### G27. Run `nori-protocol` tests in CI (S)

- **Today:** `.github/workflows/rust-ci.yml` never runs
  `cargo test -p nori-protocol`; only the schema tests in `nori-cli` run.
  (`nori-rs/nori-protocol/docs.md`.)
- **Done when:** CI runs them.

### G28. TUI log filter under `nori` (S)

- **Today:** `nori` sets up logging first, so the TUI's default filter
  (`nori_tui=info,codex_rmcp_client=info`) never applies and `nori-tui.log`
  stays empty (`docs/runbooks/debugging.md`).
- **Done when:** TUI logs reach a file when run through `nori`, or
  `nori-tui.log` is removed and the runbook says where TUI logs go.

### G29. `--skip-trust-directory` help text (S)

- **Today:** the help (`tui/src/cli.rs`) says it "automatically trusts the
  current directory". It only skips the prompt and saves nothing.
- **Done when:** the help text matches the behaviour (`docs/commands.md`).

## Needs a decision, no rule yet

These load but do nothing. No decision says whether to build, remove, or
warn on them. Do not delegate until the owner decides.

- `[hooks]` `pre_tool_call`, `post_tool_call`, `pre_agent_response` and their
  `async_` forms: parsed, never run.
- `shell_environment_policy.experimental_use_profile`: never read.
- `[cli] first_launch_complete`: written, never read.
- MCP OAuth tokens are never refreshed; expired ones are passed on with a
  warning.

## Not in this repo

Tracked here so they are not lost; they belong to nori-sessions.

- Native's vendored `native/src/shared/hostWire/host-wire.ts` is not checked
  against the pinned cli version (Host contract: Who owns what).
- The sessions bootstrap passes `-c features.multi_session=true`
  (`broker/server/bootstrap/scripts/agent-runtime/nori-managed-host.sh`); the
  key does not exist, so the flag does nothing.
- Registry gaps on the broker side: `nori.dev/session` not merged with
  `nori/session`; `noriErrorCode` and `data._nori` error shapes instead of
  `nori/error`; `_meta.nori.status` instead of `nori/sessionStatus`;
  `_nori/btw/followUp` not renamed; `nori/artifact` missing from
  `meta-keys.ts`.

Control roles are not listed: the owner set the control redesign aside, and
the host contract describes today's behaviour.
