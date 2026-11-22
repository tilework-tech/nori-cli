# Noridoc: nori-config

Path: @/nori-rs/nori-config

### Overview

`nori-config` is the sole configuration and runtime-policy boundary for the
Nori CLI, TUI, ACP host, harness, and local sandbox support.

### How it fits into the larger codebase

Frontends resolve one `NoriConfig` from `$NORI_HOME/config.toml` (default
`~/.nori/cli/config.toml`) plus explicit launch overrides, then inject it into
the harness. Session launch, resume, and probing do not reload ambient config.

### Core Implementation

The crate owns:

- agent registry and distribution configuration, including per-agent
  `[default_models]` and the optional `[[agents]].model_override`
  (`ModelOverrideToml { env, arg }`) declaring how a custom agent accepts a
  forced model id at spawn time (see below);
- `AskForApproval`, `SandboxPolicy`, `SandboxMode`, and `TrustLevel`;
- `McpServerConfig` and MCP transport configuration;
- `ShellEnvironmentPolicy` and related environment patterns;
- resolved TUI, hooks, history, notifications, worktree, cloud, and proxy
  settings; and
- comment-preserving `NoriConfigEdits` for the user-owned TOML file.

Resolution applies typed CLI overrides, raw dotted overrides, then the user
file. Project trust resolves against the effective cwd and primary git root.

`[tui] resize_reflow` controls whether the inline TUI rebuilds terminal
scrollback after width changes. It defaults to `true`; the `/settings` toggle
persists to this same key, so startup configuration and runtime changes share
one source of truth.

`NoriConfig::tool_output` (`ToolOutput::Full` by default, or `Compact`) picks
the inline view for the TUI's tool call cells; the transcript overlay always
shows the full view. It resolves from, in order: a `-c tui.tool_output=...`
command-line override, the active project's `tool_output`, `[tui]
tool_output`, then `Full`. Because `-c` overrides are merged into the parsed
`[tui]` table before resolution, the loader checks the raw overrides for
`tui.tool_output`, or a `tui` table that contains `tool_output`, to know
whether the `[tui]` value came from the command line and must beat the project
value; a `tui` table without that key leaves the project value in charge. The
result is exposed as `NoriConfig::tool_output_from_command_line`, which lets
callers tell a one-off override apart from a saved setting.

`[projects."<path>"]` entries deserialize to `ProjectConfig`, which holds
`trust_level` and `tool_output`. The active project is the entry for the exact
cwd, otherwise the entry for the primary git root (found through the git common
dir, so linked worktrees share the main repository's entry). `NoriConfigEdits`
writes both fields under the same canonicalized project key through
`set_project_trust_level` and `set_project_tool_output`; the shared helper also
re-formats a project written as an inline table so an added field stays tidy.
The TUI's directory-trust onboarding step uses both edits.

`[features]` holds opt-in behavior flags, resolved to plain booleans on
`NoriConfig`. `multi_session` (default `false`) lets the TUI hold several
sessions and move between them; it is read once at startup. `[tui]
max_sessions` resolves to `NoriConfig.max_sessions`, defaulting to
`DEFAULT_MAX_SESSIONS` (eight) and clamped to at least one; the TUI passes it
to the harness `SessionManager` as its capacity. `[tui.hotkeys]`
resolves every `HotkeyAction` to a `HotkeyBinding`, with per-action defaults
from `HotkeyAction::default_binding`. The session-move actions
(`previous_session`, `next_session`, `next_waiting_session`,
`open_session_list`, `parent_session`) always resolve, but `HotkeyAction::is_multi_session`
marks them so frontends ignore and hide them unless `multi_session` is on.

`[host]` configures the managed machine listener. `managed` and `side_sessions`
default to false. For both shared and dedicated layouts, `max_sessions` defaults
to eight process slots and must be positive. A dedicated binding can use
spare slots for several rebuilt side sessions beside its parent.
`side_confinement` is `"preferred"` or `"required"` and defaults to
`"preferred"`. It decides whether a rebuilt side may start when the machine
cannot create the OS read-only launch.
[Host configuration and admission](../cli/src/host/docs.md#managed-launch-control-v1)

`[default_models]` maps an agent slug to a model id. The loader keeps the order of dotted model overrides so the harness can resolve precedence after registering custom agents. The agent validates its model ID at launch or through ACP. `nori-acp-host` reads `model_override` to build a
`ModelInjection` strategy — `env` names an environment variable, `arg` a CLI
flag, and `env` wins if both are set — so an out-of-catalog default model can be
forced through the agent subprocess at spawn. Built-in agents with a spawn-time override get their injection channel from the [ACP registry](../acp-host/src/registry.rs). Antigravity, Grok Build, OpenCode, and Hermes Agent use their advertised ACP model options. The user-facing
`config.toml` schema, including these tables and the `[[agents]]` fields, is
described in [`config.md`](../config.md).

### API token provider resolution

The loader joins each canonical agent slug to its named API connection and
validates the protocol before the harness selects a command. The resolved
binding carries the effective protocol, endpoint, model and credential
variable name. This lets frontends load configuration without access to a
key. Codex's launch adapter reads the key for the child process.
[Resolver](src/token_providers.rs) · [Launch adapter](../acp-host/src/token_launch.rs) · [Configuration example](../config.md#named-api-token-providers)

The CLI embeds the Sessions contract byte-for-byte at version 1, revision 1.
The runtime reader accepts any revision of the supported major version.
An offline test checks that the embedded revision meets the minimum declared in
the release notes. Edit the tracked declaration when raising that minimum. The
release workflow copies it verbatim into the published notes.
[Release-note declaration](data/harness-compat-release-note.md) · [Revision check and parity tests](tests/harness_compat.rs)

Keep the source commit, embedded bytes and broker parity fixture together when
updating the contract.
[Embedded contract](data/harness-compat.json) · [Pinned Sessions source](https://github.com/tilework-tech/nori-sessions/blob/f2c787a47128f8dff118fb327906d1f09effc795/broker/server/src/shared-definitions/harness-compat.json) · [Evaluator](src/harness_compat.rs)

The evaluator accepts harness IDs and aliases declared in the contract. Resolve
the CLI launch aliases `hermes-agent` and `cursor-agent` to
`hermes` and `cursor` before querying it. The source, environment and model
determine the status, effective protocol, exclusion reason and evidence.
Each named CLI connection must declare a protocol valid for its preset. It uses local eligibility and the selected
model. A saved gateway tries its declared protocol first. A custom endpoint
uses only its declared protocol. Native sign-in and subscription-backed
sources have their own credential paths.

An exact query checks only the requested protocol. A provisional candidate
needs a protocol-specific rule to pass that check. Locally, custom Responses
is unverified for Pi and excluded for Hermes. Custom Completions is unverified
for Hermes. A broad Hermes query for OpenRouter can yield an unverified result
without a wire protocol. Such a result cannot supply a CLI binding. The ACP
registry separately checks that the selected command implements a named
connection adapter before launch. [Launch registry](../acp-host/src/registry.rs)

Upstream Codex separates a named provider from its selected model and accepts
an environment reference for the provider's key. Pi composes a model from
its provider ID, protocol and endpoint, and its auth resolver checks stored
credentials before configuration keys. Nori keeps a single explicit
environment reference. A Pi adapter consuming this contract needs a private
Pi configuration scope where a stored account cannot supersede that key.
[Codex configuration](https://github.com/openai/codex/blob/704ffed1c58ac349c129e459c50c71979dfac6b4/codex-rs/config/src/config_toml.rs#L175-L188) · [Codex credential resolution](https://github.com/openai/codex/blob/704ffed1c58ac349c129e459c50c71979dfac6b4/codex-rs/model-provider-info/src/lib.rs#L524-L542) · [Pi provider composition and auth](https://github.com/earendil-works/pi/blob/780bf31fcaeb252fa9b68df3964c735eae75bde3/packages/coding-agent/src/core/provider-composer.ts#L462-L478)

### Things to Know

- These policy types are configuration semantics, not ACP session events; they
  must not move into `nori-protocol` to avoid a dependency edge.
- MCP authentication status is computed and owned by `codex-rmcp-client`.
- There are no Codex profiles, managed preferences, or project-local config
  merge layers. Reusable agent behavior belongs to Nori Skillsets.
- The crate remains independent of `codex-core` and the deleted
  `codex-protocol` crate.

Created and maintained by Nori.
