# Noridoc: nori-providers

Path: @/nori-rs/providers

### Overview

- Reports, for each built-in ACP agent, whether it is signed in and which credentials it would find, without returning or logging secret values.
- Stores one API key per built-in agent that accepts an API-key environment variable ("saved keys"), in the OS keyring or a private file under `$NORI_HOME`.
- Passes a saved key to the agent as that environment variable when Nori launches it, unless the user already supplied the variable some other way.

### How it fits into the larger codebase

```text
nori providers [set-key|remove-key]      PreparedAgent::prepare
   (@/nori-rs/cli/src/providers.rs)      (@/nori-rs/harness/src/backend/prepared.rs)
            |                                     |
            v                                     v
   detect_all / SavedKeyStore            inject_saved_key -> AcpAgentConfig.env
            \                                     /
             +----------- nori-providers --------+
                     |                    |
          codex-keyring-store       nori-acp-host::registry
          (@/nori-rs/keyring-store)  (AgentKind, AcpAgentConfig)
```

- The CLI's `nori providers` subcommand ([`providers.rs`](../cli/src/providers.rs)) is the user-facing surface: a status table or JSON report, `set-key <agent>` (key read from stdin, never argv), and `remove-key <agent>`. It exists so the native app can show per-agent sign-in status and save a key the user enters (roadmap Phase C2, spec gap G4).
- The harness calls `inject_saved_key` during agent preparation ([`prepared.rs`](../harness/src/backend/prepared.rs)), so the TUI, `nori exec`, local host sessions, and delegated children all receive saved keys. See [`nori-harness`](../harness/docs.md).
- Managed launches (`nori host --stdio`, built from [`managed_launch.rs`](../harness/src/managed_launch.rs) with an empty child environment) do not go through that preparation step and never receive saved keys.
- Agent identity (`AgentKind`, slugs, display names, auth hints) and the launch config it mutates (`AcpAgentConfig`) come from [`nori-acp-host`](../acp-host/docs.md)'s registry. Custom `[[agents]]` entries have no `builtin_kind` and are ignored.
- Keyring access goes through the `KeyringStore` trait in [`codex-keyring-store`](../keyring-store/docs.md), so tests substitute in-memory or failing keyrings.

### Core Implementation

**Detection** ([`detect.rs`](src/detect.rs)). `detect_all` / `detect_agent(AgentKind, &DetectContext)` produce an `AgentAuthStatus` per agent: a `status` (`signed_in`, `signed_out`, `unknown`), the `credentials` found in the agent's own precedence order (the first entry is the one the agent will use), the saved-key variable if any, and the registry's auth hint. Each credential has a `kind` (`subscription`, `api_key`, `cloud`, `token`, `login`), a tagged `source` (`env`, `file`, `keychain`, `nori_saved`), and an optional non-secret `detail` such as a plan name, Codex `auth_mode`, or a provider id.

`DetectContext` injects every input: home directory, environment lookup, `Platform`, a `KeychainProbe`, and the `SavedKeyIndex`. Tests point it at fixture homes and fake keychains.

| Agent | Sources checked, in precedence order |
| --- | --- |
| Claude Code | `CLAUDE_CODE_USE_BEDROCK` / `_VERTEX` / `_FOUNDRY` (cloud), `ANTHROPIC_AUTH_TOKEN`, `ANTHROPIC_API_KEY`, saved key, `CLAUDE_CODE_OAUTH_TOKEN`, macOS keychain `Claude Code-credentials`, `.credentials.json` `claudeAiOauth` (plan from `subscriptionType`), `.claude.json` `primaryApiKey` |
| Codex | `CODEX_API_KEY`, `OPENAI_API_KEY`, saved key, `$CODEX_HOME/auth.json` (kind from `auth_mode`), macOS keychain `Codex Auth` when `config.toml` sets `cli_auth_credentials_store` to `keyring` or `auto` |
| Antigravity (`gemini` slug) | `GOOGLE_GENAI_USE_VERTEXAI`, `AGY_ADC_AUTH` (cloud), `GEMINI_API_KEY`, saved key, `GOOGLE_API_KEY` |
| Grok Build, Cursor | `XAI_API_KEY` / `CURSOR_API_KEY`, then saved key |
| OpenCode, Pi | Per-provider entries in their `auth.json` (`$XDG_DATA_HOME` or `~/.local/share/opencode/`, `~/.pi/agent/`) |
| Hermes | Nothing; always `unknown` |

**Saved keys** ([`saved.rs`](src/saved.rs)). `SavedKeyStore` saves, loads, and deletes one key per agent. `saved_key_env` in [`lib.rs`](src/lib.rs) is the single map from agent to variable; agents without one (OpenCode, Pi, Hermes) are refused. Keys must be a single non-empty line and are trimmed. Keyring entries use service `Nori Provider Keys` with the agent slug as account.

| `NORI_CREDENTIALS_STORE` | Behavior |
| --- | --- |
| `auto` (default, also unset/blank) | OS keyring; on keyring failure, warn and fall back to the file |
| `keyring` | OS keyring only; failure is an error |
| `file` | File only |

Two files live in `$NORI_HOME`: `saved-keys.json`, a non-secret index of agent slug to location (`keyring` or `file`), and `saved-keys.secret.json`, holding file-stored keys. Both are written atomically (temp file, fsync, rename) with mode 0600.

**Launch** ([`launch.rs`](src/launch.rs)). `apply_saved_key` inserts the saved key into `AcpAgentConfig.env` and returns the variable it set. `inject_saved_key` wraps it with the system keyring and `$NORI_HOME`, and turns any error (bad `NORI_CREDENTIALS_STORE`, unreadable keyring or file) into a warning that names the agent but not the value; the agent still launches.

### Things to Know

- A saved key is skipped when the variable is set to a non-blank value in the ambient environment, already present in the agent config's `env`, listed in `launch.env_remove`, or when the agent is bound to a named token provider. Environment therefore always wins over a saved key.
- For Claude Code, `ANTHROPIC_API_KEY` outranks a subscription login, so a saved key takes precedence over a Claude.ai sign-in. Detection lists the saved key above the subscription sources accordingly.
- Detection and launch read only the index unless it records a keyring entry; with nothing saved they never touch the keychain, which avoids macOS keychain prompts on every launch.
- The macOS keychain is only probed for existence (`security find-generic-password` without `-g`), never read. When `CLAUDE_CONFIG_DIR` is set, the Claude service name gets a suffix of the first 8 hex characters of the SHA-256 of that path, and `.credentials.json` and `.claude.json` are read from that directory.
- `unknown` means "nothing visible, but the agent may have a login Nori cannot inspect" (Antigravity's own keychain login, Grok, Cursor, OpenCode, Pi, Hermes, and Codex on Linux with a keyring store). Only Claude Code and Codex (outside that Linux case) report `signed_out`.
- Gemini CLI's `~/.gemini/oauth_creds.json` is intentionally not checked: the `gemini` built-in launches Antigravity (`agy`), which does not use it.
- Credential files that are missing or malformed are treated as absent. Parsed JSON is used only to pick non-secret fields.
- When a key moves from keyring to file (fallback after a keyring failure), the old keyring entry is deleted best-effort; moving to keyring removes any file copy.

Created and maintained by Nori.
