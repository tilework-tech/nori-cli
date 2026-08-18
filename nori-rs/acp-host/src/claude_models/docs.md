# Noridoc: claude_models

Path: @/nori-rs/acp-host/src/claude_models

### Overview

- Widens the model list the TUI's `/model` command offers for the Claude agent. The Claude ACP adapter (`@agentclientprotocol/claude-agent-acp`) advertises only a short curated set through `session/new` -> `configOptions`, omitting concrete models the user's account can actually run.
- Nori cannot simply append entries to the picker: the adapter **rejects** `session/set_config_option` for any model id it did not itself advertise. The list has to be widened at the source, inside Claude Code, before the adapter derives its options.
- Nori holds no Anthropic credentials (all provider auth lives inside the spawned agent subprocess), so it cannot query `/v1/models`. Instead it reads Anthropic's own **public, unauthenticated** published id list and injects it into Claude Code via a generated wrapper executable.

### How it fits into the larger codebase

```
AcpConnection::spawn()              (@/nori-rs/acp-host/src/connection)
    | AgentKind::ClaudeCode && env has CLAUDE_CODE_EXECUTABLE && NORI_HOME resolves
    v
claude_executable_override(cache_dir, claude_path)
    |
    |-- catalog.rs   two fetches issued concurrently (tokio::join!)
    |      raw.githubusercontent   SDK model.py  -> Literal[...] id union
    |      platform.claude.com     deprecations  -> "Model status" table
    |
    |-- $NORI_HOME/cache/anthropic-models.json    id cache, revalidated on read
    |
    |-- shim.rs      unix only; returns Err on every other platform
    |      $NORI_HOME/cache/claude-settings.json  } published by atomic rename
    |      $NORI_HOME/cache/claude-with-models    }
    v
cmd.env("CLAUDE_CODE_EXECUTABLE", <wrapper>)
    |
    v
claude-agent-acp -> wrapper -> claude --settings <file> "$@"
    |
    v
session/new -> configOptions (category: "model") -> TUI /model picker
```

- The only caller is `AcpConnection::spawn` in `@/nori-rs/acp-host/src/connection/acp_connection.rs`. The module is crate-private; nothing in `nori-harness` or the TUI knows it exists.
- Downstream, the widened option flows through the ordinary config-options path: `AcpBackend::config_options()` and the `/model` picker in `@/nori-rs/tui/` (see `@/nori-rs/harness/docs.md` and `@/nori-rs/tui/docs.md`).
- It also unblocks `[default_models]`. `apply_default_model` in `@/nori-rs/harness/src/backend/session_defaults.rs` validates a persisted model id against the option's advertised values and silently skips when absent, so a saved id the adapter never advertised previously never applied.
- Reuses the `CLAUDE_CODE_EXECUTABLE` env var that `@/nori-rs/acp-host/src/registry.rs` already sets for an unrelated reason (the bunx musl/glibc resolution workaround). This module only *overrides* an existing value; it never introduces one.
- Adds the crate's only outbound HTTP dependency (`reqwest`). All other network traffic in `nori-acp-host` belongs to the agent subprocess.

### Core Implementation

- `mod.rs` owns orchestration and failure policy. `resolve()` receives the HTTP client and both source URLs as parameters so tests can point at a loopback server with proxying disabled; `claude_executable_override()` is the production entry that pins Anthropic's URLs and builds a client that **does** honour ambient proxy settings, because users behind a corporate proxy still need to reach Anthropic.
- The two fetches are independent, so they are issued concurrently under `tokio::join!`. Added spawn latency is therefore bounded by one `FETCH_TIMEOUT`, not the sum of two.
- `catalog.rs` parses and filters. Parsing is deliberately shape-tolerant string scanning rather than a Python or Markdown parser, so an upstream layout change yields zero ids (which then falls through to the cache) instead of partial garbage:

| Step | Source | Behavior |
| --- | --- | --- |
| Parse ids | SDK `model.py` `Literal[...]` union | Takes odd-indexed `"`-split segments, keeps ids passing `is_model_id`, dedupes, preserves order |
| Drop retired | Deprecations "Model status" table | Scans only rows after a header line containing `Current state`, stops at the first non-table line, strips backticks from cells, drops ids whose state is present and not `Active` |
| Collapse dated | Surviving ids | Drops `…-YYYYMMDD` variants when the undated alias is also present, since both select the same model |

- Id precedence is explicit, and the cache is a fallback rather than a first choice:

| Situation | Ids used |
| --- | --- |
| Fetch succeeded and parsed to at least one id | The fetched list, which also **overwrites** the cache |
| Fetch failed, answered non-2xx, or parsed to zero ids | The cached list, after re-validating every entry with `is_model_id` |
| …and the cache is absent, corrupt, or fully invalid | None — `resolve()` returns `None` and no injection happens |
| Any id list, but deprecation filtering empties it | None — never an empty picker |

- `shim.rs` writes two files into the cache dir: a settings JSON containing only `{"availableModels": [...]}`, and an `sh` script (mode `0755`) that `exec`s the real `claude` with `--settings <file>` prepended to the adapter's own argv. Paths are single-quoted into the script so `$`, backticks and backslashes in a directory name cannot corrupt or execute.
- Both artifacts are published by writing to a pid-suffixed temp file and `rename`-ing it into place, and the write is skipped entirely when the on-disk bytes already match. Non-unix builds compile a `write_shim` that simply returns an error.

### Things to Know

- **Never produce an empty `availableModels`.** Blanking the picker is strictly worse than doing nothing. Every failure path — unreachable host, 404 page as payload, unusable cache, filtering empties the list, shim unwritable, non-unix platform — resolves to `None`, and `spawn` then leaves the adapter's own curated list untouched.
- **Atomic publication is load-bearing, not hygiene.** Concurrent nori sessions share these exact paths. A truncating write let one session read a half-written settings file and — verified empirically — produced `ETXTBSY` on both sides: on the writer (`fs::write` onto a file currently being executed) and on the executor (`execve` of a file currently being written), the latter failing agent spawn outright. Rename plus the content-equality skip removes both.
- **Windows is a deliberate no-op.** Node has refused to `spawn` a `.cmd` without a shell since CVE-2024-27980, so a `.cmd` wrapper would *break* the Claude agent rather than degrade it; the branch was also never compiled, since CI is Linux and macOS only. `write_shim` reports failure there and the adapter's own list stands.
- **There is no cache TTL**, deliberately: injection happens once per agent spawn and the payload is small. The honest consequence of the precedence table above is that on a machine that has fetched successfully at least once, a permanent upstream format change would keep serving the last known-good list indefinitely rather than surfacing as a failure.
- Cached ids are re-validated through `is_model_id` on every read, so a hand-edited, foreign, or torn cache file cannot inject arbitrary strings into the settings handed to Claude Code.
- Retired-model scanning is anchored to the "Model status" table's header for a reason: the deprecations page carries other tables whose first column is a model id (deprecation history, recommended replacements), and treating one of those as a status row would silently drop a live model from the picker.
- The deprecations page is a **refinement, not a requirement**. If only that fetch fails, the full catalog is still offered. Models *absent* from the table are kept — a model too new to be listed is not a retired one, and that is what makes freshly announced ids appear.
- No model names or aliases are hardcoded anywhere. Claude Code re-derives its own curated aliases (`opus`, `sonnet`, `haiku`, …) from the concrete ids it is handed, so nori stays out of the naming business. `--settings` **merges** into the user's existing configuration rather than replacing it, which is why auth, labels and user settings survive; it lives in a file so no JSON passes through shell quoting.
- Injection is gated on all of: `AgentKind::ClaudeCode`, `CLAUDE_CODE_EXECUTABLE` already present in the resolved agent env, and a resolvable `NORI_HOME`. When `claude` is not on PATH the registry sets no such var and this module never runs. Custom `[[agents]]` entries default to the ClaudeCode kind, so they are affected only if the user set the env var themselves.
- The list is **not entitlement-aware** — `availableModels` is an allowlist, not an access check, so a model the account cannot run may still appear and fail at prompt time (Claude Code applies some filtering of its own downstream). Claude-only by design: Codex refreshes its model list from OpenAI's server and Gemini is unaffected.

Created and maintained by Nori.
