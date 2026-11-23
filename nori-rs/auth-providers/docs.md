# Noridoc: nori-auth-providers

Path: @/nori-rs/auth-providers

### Overview

- Reports, for each built-in ACP agent, whether it is signed in and which credentials it would find, without returning or logging secret values.
- Saves credentials entered through Nori keyed by **model provider** (`anthropic`, `openai`, `google`, `xai`, `cursor`), not by agent, so one saved OpenAI key reaches every OpenAI-backed agent.
- At launch, passes each saved provider key to the agent in that agent's own variable for the provider, but only for agents the user opted in with `[auth.<agent>] prefer_api_key = true`. By default an agent's own login (for example a Claude.ai or ChatGPT subscription) always wins and saved keys are not passed.
- Recognises keys by their public prefix so the CLI can echo and confirm a key without revealing its secret part.

### How it fits into the larger codebase

```text
Nori Native app / user shell
            |  (subprocess of the bundled nori binary)
            v
nori auth [--json|set-key|remove-key|      PreparedAgent::prepare
  prefer-api-key|prefer-login]               (@/nori-rs/harness/src/backend/prepared.rs)
  (@/nori-rs/cli/src/auth.rs)                 AcpBackendConfig.prefer_api_key
TUI /api-key                                         |
  (@/nori-rs/tui/src/chatwidget/api_key.rs)          |
            |                                         v
            v                                 inject_saved_keys -> AcpAgentConfig.env
   detect_all / detect_local_agent /
   CredentialStore / masked_key
            \                                         /
             +------------- nori-auth-providers -----------+
                     |                       |
          codex-keyring-store          nori-acp-host::registry
          (@/nori-rs/keyring-store)    (AgentKind, AcpAgentConfig)
```

- The CLI's `nori auth` subcommand ([`auth.rs`](../cli/src/auth.rs)) is the stable interface for the Nori Native app: the app runs the bundled `nori` binary as a subprocess and uses `nori auth --json`, `set-key <provider>` (key on stdin, never argv), `remove-key <provider>`, and `prefer-api-key <agent> --json` / `prefer-login <agent> --json`. There is no ACP `authMethods` mapping and no custom ACP wire method for providers; the JSON output is the contract.
- The per-agent opt-in lives in `config.toml` as `[auth.<agent>]` (`AgentAuthPreference` in [`nori-config`](../nori-config/docs.md)). This crate never reads config itself: callers resolve the preference and pass it in, as `DetectContext.prefer_api_key` for detection and as the `prefer_api_key` argument of `inject_saved_keys` / `apply_saved_keys` / `detect_local_agent`.
- The harness calls `inject_saved_keys` during agent preparation ([`prepared.rs`](../harness/src/backend/prepared.rs)) with `AcpBackendConfig.prefer_api_key`, so the TUI, `nori exec`, local host sessions, and delegated children receive saved keys only for agents that prefer them. See [`nori-harness`](../harness/docs.md).
- The TUI's `/api-key` picker ([`api_key.rs`](../tui/src/chatwidget/api_key.rs)) and `nori auth prefer-api-key` both call `detect_local_agent` to find a subscription before asking the user to confirm the billing change.
- Managed launches (broker-started `nori host --stdio --launch`, built from [`managed_launch.rs`](../harness/src/managed_launch.rs) with an empty child environment) intentionally do not use saved keys. This is a design decision: the session broker's own credential management supplies credentials for those launches.
- Agent identity (`AgentKind`, slugs, aliases, display names, auth hints) and the launch config it mutates (`AcpAgentConfig`) come from [`nori-acp-host`](../acp-host/docs.md)'s registry. `AgentKind` is re-exported so the CLI and TUI need no direct `nori-acp-host` dependency for it. Custom `[[agents]]` entries have no `builtin_kind` and are ignored.
- Keyring access goes through the `KeyringStore` trait in [`codex-keyring-store`](../keyring-store/docs.md), so tests substitute in-memory or failing keyrings.
- "Provider" here is the glossary's **provider** (the model vendor), distinct from a named **token provider** ([`docs/glossary.md`](../../docs/glossary.md)). An agent bound to a token provider never receives saved keys.

### Core Implementation

**Provider routing** ([`lib.rs`](src/lib.rs)). Three tables drive everything and are shared by detection and launch:

| Table | Purpose |
| --- | --- |
| `Provider` | The providers a key can be saved for; ids match Pi's and OpenCode's `auth.json` provider ids. `Provider::parse` rejects unknown ids without echoing the input (it may be a key pasted in the wrong place) and, when given an agent slug or alias, errors and names the provider to use instead. `Provider::agent_routes()` inverts `key_routes` into the provider-to-agents map that both the report's `providers[].agents` and `set-key` output use. `base_url_env()` names the provider's endpoint-override variable (`ANTHROPIC_BASE_URL`, `OPENAI_BASE_URL`, `GOOGLE_GEMINI_BASE_URL`; none for xai and cursor). |
| `key_routes(agent)` | Which providers' saved keys an agent accepts and the variable each is passed in. |
| `env_credentials(agent)` | Every environment credential the agent honours, in its precedence order, each tagged with a provider and kind. Flag variables (such as `CLAUDE_CODE_USE_BEDROCK`) count only when `1` or `true`. |

| Agent | Saved-key routes |
| --- | --- |
| Claude Code | anthropic as `ANTHROPIC_API_KEY` |
| Codex | openai as `OPENAI_API_KEY` |
| Antigravity (`gemini` slug) | google as `GEMINI_API_KEY` |
| Grok Build, Cursor | xai as `XAI_API_KEY`, cursor as `CURSOR_API_KEY` |
| OpenCode | anthropic, openai, xai under their usual variables; google as `GOOGLE_GENERATIVE_AI_API_KEY` |
| Pi | anthropic, openai, xai under their usual variables; google as `GEMINI_API_KEY` |
| Hermes | none |

Agents with more than one route (OpenCode, Pi) are "multi-provider"; their credentials are ranked per provider rather than as one list.

**Credential store** ([`store.rs`](src/store.rs)). `CredentialStore` holds one document mapping provider id to a typed record, modelled on Pi's `auth.json`: `{"type": "api_key", "key": "..."}`. The `type` tag lets other record kinds (such as `oauth`) be added later without changing the format. `Credential::api_key` trims the key and rejects empty or multi-line input. `Credential`'s `Debug` output redacts the key. The document is kept as raw JSON, so records of types or provider ids this build does not know are preserved across rewrites but are never decoded or injected. `index()` returns a `SavedIndex` (backend plus provider-to-record-type) with no secrets; it includes known providers with any record type, and omits unknown provider ids.

`NORI_CREDENTIALS_STORE` (`STORE_BACKEND_ENV`) picks one of two backends. There is no fallback between them and no migration from one to the other.

| Value | Backend |
| --- | --- |
| unset, blank, or `file` (default) | Single file `$NORI_HOME/credentials.json`. Written via a temp file in `$NORI_HOME` (owner-only 0600), fsync, and atomic rename. |
| `keyring` | The whole JSON document as one OS keyring item (service `Nori Credentials`, account `credentials`). No file is written. Removing the last record deletes the item. |
| anything else (including the old `auto`) | Error |

`set` and `remove` are read-modify-write cycles serialized across processes (the app and the CLI) by an advisory `flock` on the `$NORI_HOME` directory itself, so the store has no lock file and no separate index file. Reads take no lock. `StoreBackend::as_str()` gives the `file` / `keyring` name used in CLI output.

**Detection** ([`detect.rs`](src/detect.rs)). `detect_all(&DetectContext)` returns an `AuthReport`; `detect_agent` builds one `AgentAuthStatus`. `DetectContext` injects every input (home directory, environment lookup, `Platform`, a `KeychainProbe`, the `SavedIndex`, and a `prefer_api_key` lookup per agent), so tests point it at fixture homes and fake keychains. `detect_local_agent` ([`local.rs`](src/local.rs)) is the one-agent convenience used by the CLI and TUI: it builds the context from the real environment, system keychain, and the store `NORI_CREDENTIALS_STORE` selects, and returns a `LocalAgentAuth` (the agent's status plus the providers it talks to that have a saved key). It errors when the store cannot be read and may block on the OS keyring.

```text
AuthReport
  store:       file | keyring
  store_error: string, only when the store could not be read
  providers: [{id, name, saved: {type} | null, agents: [{agent, env}]}]
  agents:    [{agent, name, status, credentials, saved_keys: [{provider, env}],
               prefer_api_key, notices: [{code, message}], hint}]

DetectedCredential
  kind:       subscription | api_key | cloud | token | login
  source:     env{name} | file{path} | keychain{service}
              | nori_saved{provider, env, store}
  provider:   model provider id, when known
  detail:     non-secret detail (plan name, Codex auth_mode)
  read_only:  true except keys saved through Nori
  disconnect: "unset VAR to disconnect"
              | "run `nori auth remove-key <provider>` to disconnect"
              | "sign out in <Agent> to disconnect"
```

Credentials are listed in the order the agent prefers them:

| Agent | Order |
| --- | --- |
| Claude Code | `CLAUDE_CODE_USE_BEDROCK` / `_VERTEX` / `_FOUNDRY` (cloud), `ANTHROPIC_AUTH_TOKEN`, `ANTHROPIC_API_KEY`, `CLAUDE_CODE_OAUTH_TOKEN`, saved anthropic key, macOS keychain `Claude Code-credentials`, `.credentials.json` `claudeAiOauth` (plan from `subscriptionType`), `.claude.json` `primaryApiKey` |
| Codex | `CODEX_API_KEY`, `OPENAI_API_KEY`, saved openai key, `~/.codex/auth.json` (kind from `auth_mode`), macOS keychain `Codex Auth` when `config.toml` sets `cli_auth_credentials_store` to `keyring` or `auto` |
| Antigravity | `GOOGLE_GENAI_USE_VERTEXAI`, `AGY_ADC_AUTH` (cloud), `GEMINI_API_KEY`, `GOOGLE_API_KEY`, saved google key |
| Grok Build, Cursor | `XAI_API_KEY` / `CURSOR_API_KEY`, then the saved key |
| OpenCode, Pi | Their own `auth.json` entries first (OpenCode: `$XDG_DATA_HOME` or `~/.local/share` + `opencode/auth.json`; Pi: `~/.pi/agent/auth.json`), then env credentials, then saved keys. OpenCode reads `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `GOOGLE_GENERATIVE_AI_API_KEY` / `GOOGLE_API_KEY` / `GEMINI_API_KEY`, `XAI_API_KEY`; Pi additionally reads `ANTHROPIC_AUTH_TOKEN` and `ANTHROPIC_OAUTH_TOKEN` before `ANTHROPIC_API_KEY`. |
| Hermes | Nothing |

Saved keys (`nori_saved`) appear in `credentials` only for agents whose `prefer_api_key` is true; for other agents they are omitted, so the list reflects what the agent will actually use. `saved_keys` always lists the routes the agent *could* receive.

For single-provider agents the first credential is the one the agent will use; for OpenCode and Pi the order holds per `provider`. Their `auth.json` entries carry the entry's provider id in `provider` and a kind mapped from the entry's `type` (`oauth` → subscription, `api`/`api_key` → api_key, `wellknown` → token, otherwise login).

Notice codes (the CLI table prints each as a `note:`):

| Code | When |
| --- | --- |
| `api_key_overrides_subscription` | Single-provider agents only. The first credential is an API key or token and a subscription sign-in also exists, e.g. `ANTHROPIC_API_KEY` in the environment, or a saved anthropic key Claude Code prefers, beating a Claude.ai plan; usage is then billed to the API account. |
| `saved_key_not_used` | Any agent with `prefer_api_key` false that has a saved key for a provider it talks to; one notice per such provider, naming `nori auth prefer-api-key <agent>`. |

`AgentAuthStatus::api_key_billing_warning()` returns the warning to show *before* an agent is switched to prefer saved keys (code `api_key_overrides_subscription`, naming the detected plan): present when a subscription credential is detected. It is always `None` for multi-provider agents (OpenCode, Pi), because they keep using their own `auth.json` login per provider even when given a key, so opting them in never moves billing.

**Key format** ([`key_format.rs`](src/key_format.rs)). `key_prefix(key)` returns the longest known public prefix the key starts with and the issuer it names (Anthropic, OpenAI, OpenRouter, Google, xAI, Groq; empty issuer for the shared generic `sk-`). `masked_key(key)` keeps that prefix and replaces every other character with `*`; a key with no known prefix is masked entirely. These prefixes are fixed issuer constants and carry no secret, which is why showing them is allowed. The CLI uses them for the hidden `set-key` prompt, the saved confirmation, and the wrong-issuer warning.

**Launch** ([`launch.rs`](src/launch.rs)). `apply_saved_keys(config, store, prefer_api_key, env)` returns without setting anything unless `prefer_api_key` is true. Otherwise it walks the agent's `key_routes` and, for each route, inserts the saved key into `AcpAgentConfig.env` under the route's variable, returning the variables set. A route is skipped when:

- any `env_credentials` entry for the same provider is set in the ambient environment or the agent config's `env`, or
- the provider's base-URL variable (`Provider::base_url_env`) is set non-blank in the ambient environment or the agent config's `env`, so a real key is never sent to a gateway or proxy, or
- the route's variable is listed in `launch.env_remove`; this is how one agent opts out of a saved key that others still receive.

The whole agent is skipped when it does not prefer API keys, is bound to a named token provider, or has no routes. The store is read only when at least one route survives, so agents that keep their own login never touch the store (or the keyring). `inject_saved_keys(config, nori_home, prefer_api_key, env)` wraps this with the backend chosen by `NORI_CREDENTIALS_STORE`, the system keyring, and `$NORI_HOME`, and turns any error (bad backend value, legacy files present, unreadable file or keyring) into a warning that names the agent but no value; the agent still launches.

### Things to Know

- No secrets in output: detection never returns key values, key display goes through `masked_key` (prefix plus `*`), the report and table show only variable names, file paths, keychain service names, and record types, warnings never include values, and `Credential`'s `Debug` is redacted.
- The default is the agent's own login. Saving a key changes nothing at launch until the user opts that agent in (`nori auth prefer-api-key <agent>` or the TUI's `/api-key`), which writes `[auth.<agent>] prefer_api_key = true`. The opt-in applies at the agent's next launch, not to a running session.
- Precedence is per provider: an explicit environment credential for a provider always wins over that provider's saved key, but does not block saved keys for the agent's other providers (relevant to OpenCode and Pi).
- Because `ANTHROPIC_API_KEY` outranks a Claude.ai login, opting Claude Code in to a saved anthropic key makes it bill the API account instead of a Claude.ai plan. That is why both opt-in paths show a billing warning once and remember acceptance as `[auth.<agent>] api_billing_confirmed = true`, and why `api_key_overrides_subscription` is still reported afterwards. `launch.env_remove` still withholds one route from an agent that otherwise prefers API keys.
- The report's detection reads only the ambient environment, so it does not reflect an agent's config `env`, `launch.env_remove`, base-URL override, or token-provider binding the way launch does; a key the report lists as saved for an agent may still be withheld at launch.
- The directory `flock` works on local Linux and macOS filesystems. On NFS homes Linux emulates `flock` with `fcntl` locks, which need a writable descriptor, while the store locks a read-only directory handle, so `set-key` / `remove-key` may fail there. Reads and launches are unaffected.
- With the `keyring` backend, `nori auth`, `set-key`, `remove-key`, `prefer-api-key`, `/api-key`, and every launch of an agent that prefers API keys read the keyring, which may prompt on macOS (notably after the `nori` binary changes). The harness bounds the launch lookup with a timeout, after which the agent starts without saved keys. The default `file` backend never touches the keyring.
- Legacy per-agent format: if `saved-keys.json` exists in `$NORI_HOME`, every store call errors and tells the user to delete `saved-keys.json`, `saved-keys.secret.json`, `saved-keys.lock` (and any `Nori Provider Keys` keychain items) and re-save with `set-key`. That format was never released, so there is no migration.
- `cursor` is both a provider id and an agent slug; `Provider::parse` resolves it as the provider.
- Saved keys are not added to the ACP wire log's redaction list (only named token-provider credentials are), the same as ambient API keys.
- The macOS keychain is only probed for existence (`security find-generic-password` without `-g`), never read. When `CLAUDE_CONFIG_DIR` is set, the Claude service name gets a suffix of the first 8 hex characters of the SHA-256 of that path, and `.credentials.json` and `.claude.json` are read from that directory.
- `CODEX_HOME` is ignored because Nori launches Codex without it, so only `~/.codex` counts.
- `unknown` means "nothing visible, but the agent may have a login Nori cannot inspect" (Antigravity, Grok, Cursor, OpenCode, Pi, Hermes, and Codex on Linux with a keyring store). Only Claude Code and Codex (outside that Linux case) report `signed_out`.
- Gemini CLI's `~/.gemini/oauth_creds.json` is intentionally not checked: the `gemini` built-in launches Antigravity (`agy`), which does not use it.
- Credential files that are missing or malformed are treated as absent. Parsed JSON is used only to pick non-secret fields.

Created and maintained by Nori.
