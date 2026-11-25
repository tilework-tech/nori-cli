# nori-config

Path: `nori-rs/nori-config`

## Purpose

`nori-config` loads, resolves, and edits the Nori CLI configuration. It reads
`$NORI_HOME/config.toml` (default `~/.nori/cli/config.toml`), applies command
line overrides and the active project table, and returns one resolved
`NoriConfig`. It also owns the policy types the runtime consumes (approval,
sandbox, MCP, trust, shell environment), named API connections (token
providers), and the embedded harness-compat contract. Every key, default, and
precedence rule is owned by [`docs/configuration.md`](../../docs/configuration.md);
this page does not restate them. Terms follow [`GLOSSARY.md`](../../GLOSSARY.md).

## Modules

| Module | Main items | Role |
|---|---|---|
| `loader.rs` | `find_nori_home`, `NORI_HOME_ENV`, `NORI_HOME_DIR`, `CONFIG_FILE`, `NoriConfig::load*` | Finds `NORI_HOME`, parses TOML, merges `-c` overrides, resolves the project table |
| `types/mod.rs` | `NoriConfigToml` (raw), `NoriConfig` (resolved), `NoriConfigOverrides`, `HostConfig`, `TuiConfig`, agent and distribution tables | The schema of `config.toml` and the resolved shape |
| `config_types.rs` | `McpServerConfig`, `McpServerTransportConfig`, `SandboxMode`, `TrustLevel`, `ShellEnvironmentPolicy` | Policy value types |
| `policy.rs` | `AskForApproval`, `SandboxPolicy`, `WritableRoot` | Approval and sandbox policy |
| `edits.rs` | `NoriConfigEdits` | Comment-preserving writes to `config.toml` (`toml_edit`) |
| `auth.rs` | `AgentAuthPreference` | `[auth.<agent>]`: own login or saved key |
| `token_providers.rs` | `TokenProvider`, `Protocol` | Named API connections; a binding holds metadata and an env reference, never a secret |
| `legacy_pi.rs` | | Reads Pi provider tables in the older per-agent format |
| `harness_compat.rs` | `embedded()`, `Contract`, `SOURCE_COMMIT` | Harness eligibility from the Sessions contract, embedded from `data/harness-compat.json` |
| `git_root.rs` | `resolve_root_git_project_for_trust` | Repository root used to look up project trust |

```text
config.toml --load_toml--> NoriConfigToml --from_toml--> NoriConfig
     ^           ^                              ^
     |           |                              |
NoriConfigEdits  -c key=value              typed overrides (agent, cwd,
(comment-safe)   (raw_overrides)           approval, sandbox, writable roots)
```

## Dependency direction

```text
nori-acp-host, nori-harness, nori-host-runtime, nori-tui, nori-cli
                          |
                          v
                     nori-config --> serde, toml, toml_edit, dirs, url, regex, wildmatch
```

`nori-config` depends on no other workspace crate. In particular it does not
depend on `nori-protocol` or any `codex-*` crate.

## Invariants

- Resolve once, then pass `Arc<NoriConfig>` down. The session runtime and the
  ACP host take a resolved config; they do not reload ambient config while
  they prepare, refresh, launch, or resume an agent. The host reloads only to
  apply a session folder's project table (`HostService` in
  `nori-rs/host-runtime`).
- Keep policy types here, not in `nori-protocol`. They are configuration
  semantics, not session events.
- Write user config only through `NoriConfigEdits`. It keeps comments and
  unrelated keys.
- `NORI_HOME` is Nori's only home. Do not read or write `~/.codex` or
  `CODEX_HOME` from config code. Reading the Codex agent's own `~/.codex` for
  that agent is allowed elsewhere.
- No Codex profiles: `profile`, `[profiles]`, and the legacy `model` key are
  not supported. Reusable agent behavior belongs to Nori Skillsets.
- A token provider binding holds metadata and an environment variable name.
  Never store a secret value in config types.
- `data/harness-compat.json` is a copy from `nori-sessions`
  (`broker/server/src/shared-definitions/harness-compat.json`). Update the
  copy, `SOURCE_COMMIT`, and `data/harness-compat-release-note.md` together.
  The harness-compat rules themselves are owned by `nori-sessions`.
- `sandbox_mode`, `[sandbox_workspace_write]`, and the derived
  `SandboxPolicy` are parsed and carried to the runtime but are not enforced
  on the ACP path yet. Keep them. Per-harness sandboxing is tracked in
  monorepo PR #263. See [`docs/permissions.md`](../../docs/permissions.md).
- Tests that set `NORI_HOME` or other process environment use
  `#[serial]` (`serial_test`). Prefer passing paths through
  `load_from_path*` instead.

## Tests

| What | Where |
|---|---|
| Loader, defaults, `find_nori_home` | `#[cfg(test)]` in `src/lib.rs`, `src/loader.rs` |
| Type parsing and resolution | `src/types/tests.rs` |
| Load behavior end to end | `tests/config_behavior.rs`, `tests/tool_output_max_lines.rs` |
| Harness-compat copy, release floor, eligibility | `tests/harness_compat.rs` |
| Token providers and legacy Pi tables | `tests/token_providers.rs`, `tests/legacy_pi_connection.rs` |

Run:

```sh
cd nori-rs && cargo test -p nori-config
```

A change here affects every product crate. Run `cargo test` for the workspace
afterwards.

## Where `main` differs

- c09: the MCP keys `startup_timeout_sec`, `tool_timeout_sec`,
  `enabled_tools`, and `disabled_tools` parse into `McpServerConfig`, but no
  product crate reads them and no warning is logged.
- c08: the MCP OAuth file fallback still resolves `CODEX_HOME` or `~/.codex`
  (`nori-rs/rmcp-client/src/find_codex_home.rs`), not `NORI_HOME`.
