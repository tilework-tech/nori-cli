# Nori CLI ACP Roadmap

This roadmap is about Nori's ACP client work, not ACP in the abstract. The goal is to keep the client simple, fast, and predictable while we fill in the highest-value protocol features.

> [!NOTE]
> Read this document in two passes: start with the product roadmap for what Nori users and maintainers should expect, then use the ACP feature appendix for protocol status and coverage detail.

## Product Roadmap

The product priorities and Nori coverage below are carried forward from the previous roadmap and await a separate implementation audit. Upstream ACP references were checked on September 10, 2026.

Status key: `✅ done` working well, `🔵 now` current focus, `🟡 next` near-term, `⚪ later` planned but not near-term.

At a high level:

```text
done                          now                          next                         later
 ●─────────────────────────────●────────────────────────────●────────────────────────────●──────────────→
core loop                     session forking              session config               agent auth
local agent registry          images/resources             official agent registry      multi-session
session lifecycle             message queue                                             unstable ACP
```

| Product area          | Status     | User value                         | Next work                                                                                                 | Maintainer note                                                                                                                                |
| --------------------- | ---------- | ---------------------------------- | --------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| File attachments      | `✅ done`  | users can reference code and docs  | keep `@` path reference behavior stable                                                                   | agents read referenced paths through ACP filesystem requests                                                                                   |
| Local agent registry  | `✅ done`  | custom agents can be registered    | keep local config registration simple                                                                     | ACP Registry should complement this, not replace it yet                                                                                        |
| Session lifecycle     | `✅ done`  | navigate previous sessions         | keep behavior stable while forking work continues                                                         | `load`, `list`, `resume`, lazy indexing, and `undo` are already working                                                                        |
| Session continuity    | `✅ done`  | users can continue prior work      | keep resume listing, speed, and metadata stable                                                           | context/usage metadata and `session_info_update` are finished                                                                                  |
| Session forking       | `🔵 now`   | sessions can branch and form trees | fork-at-cursor and tree-oriented flows (track [cursor proposal #2114][pr-2114], replacing [#629][pr-629]) | branch-at-head shipped via capability-gated `session/fork`; upstream Claude and Codex adapters support forks, subject to the installed version |
| Image attachments     | `🔵 now`   | users can send visual context      | capability-aware image routing and transcript fidelity                                                    | images reach ACP, but the path needs polish                                                                                                    |
| Queued messages       | `🔵 now`   | users can keep typing during work  | finish backend handling for queued turns                                                                  | TUI support exists, but ACP backend behavior needs tightening                                                                                  |
| Session configuration | `✅ done`  | agents can expose useful controls  | custom options like thinking/effort level and plan/build modes                                            | keep this driven by agent-provided config, not hardcoding                                                                                      |
| Agent discovery       | `🟡 next`  | custom agents are easier to find   | support the official ACP Registry                                                                         | local config registration already works                                                                                                        |
| Agent-driven auth     | `⚪ later` | agents can own login/logout flows  | auth methods and `logout`                                                                                 | wait until lifecycle, config, and registry are settled                                                                                         |
| Multi-session UX      | `⚪ later` | users can move across active work  | multi-session support and navigation                                                                      | keep this separate from single-session lifecycle cleanup                                                                                       |
| Steering messages     | `⚪ later` | users can redirect an active turn  | track the open queue/steer proposal [#1261][pr-1261]                                                      | v2 injection schema is proposed in [#2043][pr-2043], not merged                                                                                |
| Subagent UX           | `⚪ later` | group subagent updates             | track the open subagent proposal [#1992][pr-1992]                                                         | upstream Claude/Codex implementations exist; protocol proposal remains open                                                                    |
| Experimental ACP      | `⚪ later` | adopt new protocol ideas carefully | NES, provider configuration, compaction, notices, and v2                                                  | do not let draft features complicate the core client                                                                                           |

## ACP Feature Appendix

Upstream snapshot: **September 10, 2026**, protocol repository [`edc0875`][acp-snapshot]. **ACP v1 is stable; v2 is draft.** Package versions, RFD stages, and Nori implementation status are independent.

- **Stable v1** means a protocol feature is standardized; optional methods still require the relevant capability.
- **Draft / Active / Preview / Completed** follow the [upstream RFD process][rfd-about]. Active means current maintainer focus, not a stability commitment. No published RFD is in Preview at this snapshot.
- **Landed experimental** means schema or SDK implementation exists behind experimental support; an open implementation PR is not landed support.
- **Nori coverage (prior)** preserves the previous roadmap's claims, not a fresh implementation audit. **Not audited** marks newly inventoried features. These labels do not establish new priorities or lack of support.

### Stable v1 and client behavior

| Area               | Feature and upstream reference                                                                                 | Protocol status / semantics                                                                                    | Nori coverage (prior)             |
| ------------------ | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | --------------------------------- |
| Connection         | [Initialization][v1-initialization], implementation info, version and capability negotiation                   | Stable; negotiate wire version independently of SDK version                                                    | Not audited                       |
| Core loop          | [Session creation][v1-session-setup], [prompt, updates, stop reasons and session cancellation][v1-prompt-turn] | Stable; v1 prompt response ends the turn                                                                       | Not audited                       |
| Streaming          | [User, agent, and thought content][v1-content]                                                                 | Stable text baseline; preserve content type and ordering                                                       | Not audited                       |
| Tools              | [Tool calls, updates, diffs, locations, raw input/output and permissions][v1-tool-calls]                       | Stable; permission requests are client callbacks                                                               | Not audited                       |
| Plans and commands | [Agent plans][v1-agent-plan], [available commands][v1-slash-commands]                                          | Stable; command UI is client behavior                                                                          | Not audited                       |
| Session lifecycle  | [session/load][v1-session-setup]                                                                               | Stable optional method; restores with history replay                                                           | Done                              |
| Session lifecycle  | [session/list][rfd-session-list]                                                                               | Stable / Completed; paginated session discovery                                                                | Done                              |
| Session lifecycle  | [session/resume][rfd-session-resume]                                                                           | Stable / Completed; restores without history replay                                                            | Done                              |
| Session lifecycle  | [session/close][rfd-session-close]                                                                             | Stable / Completed; ends active handling without deleting persisted history                                    | Handled                           |
| Session lifecycle  | [session/delete][rfd-session-delete]                                                                           | Stable / Completed; distinct from close                                                                        | Not planned near term             |
| Session metadata   | [session_info_update][rfd-session-info-update]                                                                 | Stable / Completed; session title and metadata updates                                                         | Done                              |
| Session metadata   | [usage_update][rfd-session-usage]                                                                              | Stable / Completed; current context size/utilization and optional cumulative session cost                      | Done                              |
| Message identity   | [messageId][rfd-message-id] on user, agent and thought chunks                                                  | Stable / Completed; optional in v1, generated by the agent; ordinary prompt request/response has no message ID | Partial                           |
| Workspace          | [additionalDirectories][rfd-additional-directories]                                                            | Stable / Completed; capability-gated additional roots on session setup requests                                | Partial                           |
| Filesystem         | [fs/read_text_file][v1-file-system]                                                                            | Stable optional client capability; `@` path selection is Nori UI, not a wire method                            | Path references done              |
| Filesystem         | [fs/write_text_file][v1-file-system]                                                                           | Stable optional client capability                                                                              | Not audited                       |
| Terminals          | [Create, output, wait, kill and release][v1-terminals]                                                         | Stable optional client execution capability                                                                    | Not audited                       |
| Resources          | [Resource links and embedded resources][v1-content]                                                            | Stable content forms; embedded prompt context requires `embeddedContext`                                       | Todo                              |
| Media              | [Image and audio content][v1-content]                                                                          | Stable content forms; prompt support is capability-gated                                                       | Images partial; audio not audited |
| Configuration      | [Session config options][rfd-session-config-options] and [session modes][v1-session-modes]                     | Config options stable / Completed; legacy `session/set_mode` remains in v1                                     | Config options done               |
| Configuration      | [Boolean config options][rfd-boolean-config-option]                                                            | Stable / Completed; v1 clients explicitly advertise boolean support                                            | Near term                         |
| Configuration      | [model_config category][rfd-model-config-category]                                                             | Stable / Completed; model parameters alongside model selection                                                 | Not audited                       |
| Authentication     | [Baseline authenticate and terminal authentication][rfd-auth-methods]                                          | Baseline auth stable; Terminal Authentication Completed; run configured agent interactively, then reconnect    | Planned                           |
| Authentication     | [logout][rfd-logout-method]                                                                                    | Stable / Completed; optional v1 auth capability                                                                | Planned                           |
| User input         | [Elicitation][rfd-elicitation]                                                                                 | Stable / Completed; form and URL modes, `elicitation/create` and URL completion notification                   | Later                             |
| Request control    | [$/cancel_request][rfd-request-cancellation]                                                                   | Stable / Completed; bidirectional per-request cancellation, separate from `session/cancel`                     | Not audited                       |
| MCP                | [Session MCP server configuration][v1-session-setup]                                                           | Stable stdio server configuration; HTTP/SSE support requires agent capabilities                                | Not audited                       |
| Extensions         | [Custom methods/capabilities and \_meta][v1-extensibility]; [metadata propagation][rfd-meta-propagation]       | Stable extension surface; propagation conventions Completed                                                    | Not audited                       |
| Discovery          | [ACP Registry][rfd-acp-agent-registry]                                                                         | Stable / Completed; manifest/catalog format, not an ACP RPC                                                    | Near term                         |
| Turn control       | Local queued messages                                                                                          | Client behavior; protocol injection tracked below                                                              | In progress                       |
| Session editing    | Undo, rewind, checkpoints                                                                                      | No stable first-class v1 rewind/checkpoint API; proposals tracked below                                        | Local only                        |

Stable session-level usage does **not** stabilize end-turn token accounting. The [end-turn RFD][rfd-end-turn-token-usage] remains Draft, with conflicting per-turn/cumulative wording tracked in [issue #1860][issue-1860] and open [PR #2061][pr-2061]. `messageId` is agent-owned; the previous appendix's `userMessageId` wording does not describe the current protocol.

### Published draft features and implementation state

All twelve RFDs below are in **Draft**. Implementation status is checked against the [schema feature gates][schema-features], [schema changelog][schema-changelog], and SDK releases.

| Feature                                                  | Upstream implementation / limitation                                                                                                     | Nori coverage (prior)                                      |
| -------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------- |
| [Session fork][rfd-session-fork]                         | Landed experimental `session/fork`; fork at a selected boundary is a separate cursor proposal                                            | Branch-at-head done; cursor support awaits upstream design |
| [Agent extensions via ACP proxies][rfd-proxy-chains]     | Draft design with Rust SDK proxy/conductor implementations; SDK availability does not complete the RFD                                   | Not planned near term                                      |
| [MCP-over-ACP][rfd-mcp-over-acp]                         | Landed experimental MCP routing through ACP channels                                                                                     | Not audited                                                |
| [End-turn token usage][rfd-end-turn-token-usage]         | Landed experimental accounting shape; semantics still under discussion                                                                   | Not audited                                                |
| [Deleted-file diff metadata][rfd-diff-delete]            | RFD-only v1 `deleted` flag; v2 structured file changes are separate                                                                      | Not planned near term                                      |
| [Next Edit Suggestions (NES)][rfd-next-edit-suggestions] | Landed experimental surface                                                                                                              | Later                                                      |
| [Configurable LLM providers][rfd-custom-llm-endpoint]    | Landed experimental `providers/list`, `providers/set`, `providers/disable`; open [#1977][pr-1977] proposes batch update/restore redesign | Not planned near term                                      |
| [Plan operations][rfd-plan-operations]                   | Landed experimental plan IDs, item/file/markdown formats and removal                                                                     | Not audited                                                |
| [Tool call name][rfd-tool-call-name]                     | Landed experimental programmatic `name`, distinct from display title and invocation ID                                                   | Not audited                                                |
| [Authentication state query][rfd-get-auth-state]         | RFD-only `auth/status`; separate from adapter-specific auth extensions                                                                   | Not audited                                                |
| [Session compaction][rfd-session-compaction]             | Landed experimental lifecycle and summary updates; included in schema crate 1.7.0                                                        | Not audited                                                |
| [Session notices][rfd-session-notices]                   | Experimental schema [merged in #2004][pr-2004] after the 1.7.0 release; transient notices are not durable conversation entries           | Not audited                                                |

### Active remote transport design

The [Streamable HTTP & WebSocket Transport RFD][rfd-streamable-http-websocket-transport] is **Active** and targets additive v1 support. Standard local transport remains [stdio][v1-transports]; a custom bridge or SDK transport does not by itself establish conformance to this RFD.

- Proposed `/acp` transport supports HTTP/2 POST plus connection/session SSE GET streams, or a WebSocket upgrade. Remote HTTP clients are proposed to support both profiles.
- Connection and session identities route responses, notifications and agent-to-client requests. Cookies provide connection affinity; HTTP connection teardown is separate from deleting a session.
- The v1 design leaves retries, liveness and reconnect handling to implementations and does not replay in-flight messages missed during disconnection.
- Stream resumption, sequencing and standardized keepalive are future design goals, not stable v1 guarantees. Evaluate those goals alongside v2 and the open cursor/attach proposals.

Nori remote transport implementation and conformance are reserved for the separate implementation audit.

### ACP v2 draft and migration

The [v2 umbrella RFD][rfd-v2-overview] and its eleven constituent RFDs are **Active**, while the wire protocol itself remains **draft**. The [v2 announcement][v2-announcement] and [migration guide][v2-migration] recommend explicit version negotiation and experimental opt-in while retaining v1 support. This section tracks upstream changes; it does not claim Nori v2 support.

| Active RFD                                                                             | Client-facing change                                                                                                                                     |
| -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------- |
| [Prompt lifecycle][rfd-v2-prompt]                                                      | Prompt response acknowledges acceptance; foreground `state_update` reports running, idle or requires-action state. Updates can outlive a prompt request. |
| [Enum extensions][rfd-v2-enum-variant-extension]                                       | Forward-compatible informational variants and `_`-prefixed implementation extensions with defined fallback behavior.                                     |
| [Required session methods][rfd-v2-required-session-methods]                            | Agents advertising sessions must provide list, resume and close as part of the core lifecycle.                                                           |
| [Resume replay][rfd-v2-session-resume-replay]                                          | Replace `session/load` with `session/resume`; optional `replayFrom` requests history replay.                                                             |
| [Client filesystem/terminal execution][rfd-v2-client-filesystem-terminal-capabilities] | Remove standard client `fs/*` and terminal execution APIs/capabilities from v2; v1 retains them.                                                         |
| [Terminal output][rfd-v2-terminal-output]                                              | Agent-owned, display-only terminal state and byte chunks, with replay and exit information.                                                              |
| [Plan variants][rfd-v2-plan-variants]                                                  | Item-based `plan_update` replaces v1 `plan`; broader markdown/file/removal operations remain experimental.                                               |
| [Tool-call updates][rfd-v2-tool-call-updates]                                          | Unified ID-addressed upserts and streamed content; omitted fields preserve, null clears, values replace.                                                 |
| [Diff file states][rfd-v2-diff-file-states]                                            | Structured add/delete/modify/move/copy and non-text changes, with optional renderable patch content.                                                     |
| [Permission requests][rfd-v2-permission-requests]                                      | Required title and optional description/subject decouple permission copy from tool-call state.                                                           |
| [Message updates/chunks][rfd-v2-message-updates]                                       | Required agent-owned IDs, whole-message upserts and appended chunks support replay and edits.                                                            |

The migration also restructures initialization into `info`/`capabilities`, renames authentication methods to `auth/login` and `auth/logout`, consolidates modes into configuration options, and changes typed configuration/content fields. SDK major versions do not indicate that v2 has stabilized.

### Open proposal watchlist

These PRs were **open and unmerged** at the snapshot. They are proposals or proposed implementations, not completed RFDs, stable capabilities, or Nori commitments.

| Area                    | Proposal / relationship                                                                                                                                                        |
| ----------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Session positions       | [Cursors #2114][pr-2114] supplies opaque operation-valid boundaries; author-designated replacement for [fork-at-message #629][pr-629], which is also still open.               |
| Queueing and steering   | [session/inject #1261][pr-1261] and [v2 schema #2043][pr-2043]: queue/steer plus revoke/replace pending input.                                                                 |
| Subagents               | [Subagent sessions #1992][pr-1992] includes proposed schema; [discovery/delegation #855][pr-855] is another open design. Adapter implementations do not imply standardization. |
| History editing         | [Rewind/edit #1321][pr-1321] targets v2; conversation truncation is distinct from filesystem rollback.                                                                         |
| Shared sessions         | [Multi-client attach #533][pr-533], [remote-session support #442][pr-442], [session/status #986][pr-986], [ready signal #419][pr-419].                                         |
| Session metadata        | [Client-set titles #1987][pr-1987]; existing `session_info_update` reports agent-authored metadata.                                                                            |
| Permission UX           | [Free-form feedback #1983][pr-1983], [interactive file-write feedback #895][pr-895].                                                                                           |
| Rich UI                 | [Embedded views #1849][pr-1849] proposes a negotiated resource, sandbox and interaction contract.                                                                              |
| Model tracing           | [Model-request relationships #2092][pr-2092] proposes request-level causality, complementing compaction/subagent events.                                                       |
| Editor/tools            | [Client LSP #1292][pr-1292], [requested tool categories #1302][pr-1302], [dynamic MCP updates #582][pr-582].                                                                   |
| Context                 | [Client system prompt #1237][pr-1237], [runtime prompt context #874][pr-874], [workspace state #832][pr-832].                                                                  |
| Input/discovery/logging | [Session input options #393][pr-393], [agent skills list #370][pr-370], [agent-to-client logging #392][pr-392].                                                                |
| Draft refinements       | [Provider batch update/restore #1977][pr-1977], [token accounting wording #2061][pr-2061].                                                                                     |

### SDKs, registry and retired designs

- The [Rust SDK based on SACP RFD][rfd-rust-sdk-v1] is **Completed**. Upstream releases checked: [Rust SDK 2.1.0][rust-release], [TypeScript SDK 1.4.0][ts-release], and [schema crate 1.7.0][schema-changelog]. Runtime SDK, schema package and negotiated wire protocol have separate versions. This roadmap update does not upgrade Nori dependencies.
- The [Registry RFD][rfd-acp-agent-registry] is **Completed**. The [registry repository][registry] publishes a catalog with documented manifests and distribution formats. Discovery does not replace runtime capability negotiation or local registration.
- [Codex ACP 1.8.0 added session forks on September 1, 2026][codex-changelog]; 1.7.0 added subagent sessions on August 27. [Claude adapter source][claude-source] also advertises forks and subagents. Installed adapter versions and negotiated capabilities determine availability.
- The [RFD process proposal][rfd-introduce-rfd-process] is **Completed**; [lifecycle updates][rfd-updates] record the later introduction of Active and removals. All published RFDs at the snapshot are represented above, including process/SDK work that is not a wire feature.
- Removed designs: experimental `session/set_model` and model response fields (use config options); the Agent Telemetry Export RFD; and the explicit `env_var` authentication variant. See [RFD updates][rfd-updates].
- Closed proposals are not pending commitments: [workspace trust #1489][pr-1489], [sandbox capability/policy #1063][pr-1063], and [tool interception #2068][pr-2068]. Recheck upstream if these designs are revisited.

## Notes

- Upstream ACP references were checked on September 10, 2026. Product priorities and Nori coverage remain the prior roadmap's baseline pending the separate recent-work audit.
- "Done" means done enough for initial usage within Nori, not necessarily that every implementation detail is perfect.
- We will keep preferring fewer special cases and more native ACP behavior as we iterate.

[acp-snapshot]: https://github.com/agentclientprotocol/agent-client-protocol/tree/edc0875ac0f9f18e2a41d7c5b441407e97fd9698
[claude-source]: https://github.com/agentclientprotocol/claude-agent-acp/blob/main/src/acp-agent.ts
[codex-changelog]: https://github.com/agentclientprotocol/codex-acp/blob/main/CHANGELOG.md
[issue-1860]: https://github.com/agentclientprotocol/agent-client-protocol/issues/1860
[pr-1063]: https://github.com/agentclientprotocol/agent-client-protocol/pull/1063
[pr-1237]: https://github.com/agentclientprotocol/agent-client-protocol/pull/1237
[pr-1261]: https://github.com/agentclientprotocol/agent-client-protocol/pull/1261
[pr-1292]: https://github.com/agentclientprotocol/agent-client-protocol/pull/1292
[pr-1302]: https://github.com/agentclientprotocol/agent-client-protocol/pull/1302
[pr-1321]: https://github.com/agentclientprotocol/agent-client-protocol/pull/1321
[pr-1489]: https://github.com/agentclientprotocol/agent-client-protocol/pull/1489
[pr-1849]: https://github.com/agentclientprotocol/agent-client-protocol/pull/1849
[pr-1977]: https://github.com/agentclientprotocol/agent-client-protocol/pull/1977
[pr-1983]: https://github.com/agentclientprotocol/agent-client-protocol/pull/1983
[pr-1987]: https://github.com/agentclientprotocol/agent-client-protocol/pull/1987
[pr-1992]: https://github.com/agentclientprotocol/agent-client-protocol/pull/1992
[pr-2004]: https://github.com/agentclientprotocol/agent-client-protocol/pull/2004
[pr-2043]: https://github.com/agentclientprotocol/agent-client-protocol/pull/2043
[pr-2061]: https://github.com/agentclientprotocol/agent-client-protocol/pull/2061
[pr-2068]: https://github.com/agentclientprotocol/agent-client-protocol/pull/2068
[pr-2092]: https://github.com/agentclientprotocol/agent-client-protocol/pull/2092
[pr-2114]: https://github.com/agentclientprotocol/agent-client-protocol/pull/2114
[pr-370]: https://github.com/agentclientprotocol/agent-client-protocol/pull/370
[pr-392]: https://github.com/agentclientprotocol/agent-client-protocol/pull/392
[pr-393]: https://github.com/agentclientprotocol/agent-client-protocol/pull/393
[pr-419]: https://github.com/agentclientprotocol/agent-client-protocol/pull/419
[pr-442]: https://github.com/agentclientprotocol/agent-client-protocol/pull/442
[pr-533]: https://github.com/agentclientprotocol/agent-client-protocol/pull/533
[pr-582]: https://github.com/agentclientprotocol/agent-client-protocol/pull/582
[pr-629]: https://github.com/agentclientprotocol/agent-client-protocol/pull/629
[pr-832]: https://github.com/agentclientprotocol/agent-client-protocol/pull/832
[pr-855]: https://github.com/agentclientprotocol/agent-client-protocol/pull/855
[pr-874]: https://github.com/agentclientprotocol/agent-client-protocol/pull/874
[pr-895]: https://github.com/agentclientprotocol/agent-client-protocol/pull/895
[pr-986]: https://github.com/agentclientprotocol/agent-client-protocol/pull/986
[registry]: https://github.com/agentclientprotocol/registry
[rfd-about]: https://agentclientprotocol.com/rfds/about
[rfd-acp-agent-registry]: https://agentclientprotocol.com/rfds/acp-agent-registry
[rfd-additional-directories]: https://agentclientprotocol.com/rfds/additional-directories
[rfd-auth-methods]: https://agentclientprotocol.com/rfds/auth-methods
[rfd-boolean-config-option]: https://agentclientprotocol.com/rfds/boolean-config-option
[rfd-custom-llm-endpoint]: https://agentclientprotocol.com/rfds/custom-llm-endpoint
[rfd-diff-delete]: https://agentclientprotocol.com/rfds/diff-delete
[rfd-elicitation]: https://agentclientprotocol.com/rfds/elicitation
[rfd-end-turn-token-usage]: https://agentclientprotocol.com/rfds/end-turn-token-usage
[rfd-get-auth-state]: https://agentclientprotocol.com/rfds/get-auth-state
[rfd-introduce-rfd-process]: https://agentclientprotocol.com/rfds/introduce-rfd-process
[rfd-logout-method]: https://agentclientprotocol.com/rfds/logout-method
[rfd-mcp-over-acp]: https://agentclientprotocol.com/rfds/mcp-over-acp
[rfd-message-id]: https://agentclientprotocol.com/rfds/message-id
[rfd-meta-propagation]: https://agentclientprotocol.com/rfds/meta-propagation
[rfd-model-config-category]: https://agentclientprotocol.com/rfds/model-config-category
[rfd-next-edit-suggestions]: https://agentclientprotocol.com/rfds/next-edit-suggestions
[rfd-plan-operations]: https://agentclientprotocol.com/rfds/plan-operations
[rfd-proxy-chains]: https://agentclientprotocol.com/rfds/proxy-chains
[rfd-request-cancellation]: https://agentclientprotocol.com/rfds/request-cancellation
[rfd-rust-sdk-v1]: https://agentclientprotocol.com/rfds/rust-sdk-v1
[rfd-session-close]: https://agentclientprotocol.com/rfds/session-close
[rfd-session-compaction]: https://agentclientprotocol.com/rfds/session-compaction
[rfd-session-config-options]: https://agentclientprotocol.com/rfds/session-config-options
[rfd-session-delete]: https://agentclientprotocol.com/rfds/session-delete
[rfd-session-fork]: https://agentclientprotocol.com/rfds/session-fork
[rfd-session-info-update]: https://agentclientprotocol.com/rfds/session-info-update
[rfd-session-list]: https://agentclientprotocol.com/rfds/session-list
[rfd-session-notices]: https://agentclientprotocol.com/rfds/session-notices
[rfd-session-resume]: https://agentclientprotocol.com/rfds/session-resume
[rfd-session-usage]: https://agentclientprotocol.com/rfds/session-usage
[rfd-streamable-http-websocket-transport]: https://agentclientprotocol.com/rfds/streamable-http-websocket-transport
[rfd-tool-call-name]: https://agentclientprotocol.com/rfds/tool-call-name
[rfd-updates]: https://agentclientprotocol.com/rfds/updates
[rfd-v2-client-filesystem-terminal-capabilities]: https://agentclientprotocol.com/rfds/v2/client-filesystem-terminal-capabilities
[rfd-v2-diff-file-states]: https://agentclientprotocol.com/rfds/v2/diff-file-states
[rfd-v2-enum-variant-extension]: https://agentclientprotocol.com/rfds/v2/enum-variant-extension
[rfd-v2-message-updates]: https://agentclientprotocol.com/rfds/v2/message-updates
[rfd-v2-overview]: https://agentclientprotocol.com/rfds/v2/overview
[rfd-v2-permission-requests]: https://agentclientprotocol.com/rfds/v2/permission-requests
[rfd-v2-plan-variants]: https://agentclientprotocol.com/rfds/v2/plan-variants
[rfd-v2-prompt]: https://agentclientprotocol.com/rfds/v2/prompt
[rfd-v2-required-session-methods]: https://agentclientprotocol.com/rfds/v2/required-session-methods
[rfd-v2-session-resume-replay]: https://agentclientprotocol.com/rfds/v2/session-resume-replay
[rfd-v2-terminal-output]: https://agentclientprotocol.com/rfds/v2/terminal-output
[rfd-v2-tool-call-updates]: https://agentclientprotocol.com/rfds/v2/tool-call-updates
[rust-release]: https://github.com/agentclientprotocol/rust-sdk/releases/tag/v2.1.0
[schema-changelog]: https://github.com/agentclientprotocol/agent-client-protocol/blob/edc0875ac0f9f18e2a41d7c5b441407e97fd9698/CHANGELOG.md
[schema-features]: https://github.com/agentclientprotocol/agent-client-protocol/blob/edc0875ac0f9f18e2a41d7c5b441407e97fd9698/agent-client-protocol-schema/Cargo.toml
[ts-release]: https://github.com/agentclientprotocol/typescript-sdk/releases/tag/v1.4.0
[v1-agent-plan]: https://agentclientprotocol.com/protocol/v1/agent-plan
[v1-content]: https://agentclientprotocol.com/protocol/v1/content
[v1-extensibility]: https://agentclientprotocol.com/protocol/v1/extensibility
[v1-file-system]: https://agentclientprotocol.com/protocol/v1/file-system
[v1-initialization]: https://agentclientprotocol.com/protocol/v1/initialization
[v1-prompt-turn]: https://agentclientprotocol.com/protocol/v1/prompt-turn
[v1-session-modes]: https://agentclientprotocol.com/protocol/v1/session-modes
[v1-session-setup]: https://agentclientprotocol.com/protocol/v1/session-setup
[v1-slash-commands]: https://agentclientprotocol.com/protocol/v1/slash-commands
[v1-terminals]: https://agentclientprotocol.com/protocol/v1/terminals
[v1-tool-calls]: https://agentclientprotocol.com/protocol/v1/tool-calls
[v1-transports]: https://agentclientprotocol.com/protocol/v1/transports
[v2-announcement]: https://agentclientprotocol.com/announcements/acp-v2-draft
[v2-migration]: https://agentclientprotocol.com/protocol/v2/migration
