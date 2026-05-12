# Nori CLI ACP Roadmap

This roadmap is about Nori's ACP client work, not ACP in the abstract. The goal is to keep the client simple, fast, and predictable while we fill in the highest-value protocol features.

> [!NOTE]
> Read this document in two passes: start with the product roadmap for what Nori users and maintainers should expect, then use the ACP feature appendix for protocol status and coverage detail.

## Product Roadmap

Status key: `✅ done` working well, `🔵 now` current focus, `🟡 next` near-term, `⚪ later` planned but not near-term.

At a high level:

```text
done                          now                          next                         later
 ●─────────────────────────────●────────────────────────────●────────────────────────────●──────────────→
core loop                     session forking              session config               agent auth
local agent registry          images/resources             official agent registry      multi-session
session lifecycle             message queue                                             unstable ACP
```

| Product area          | Status     | User value                         | Next work                                                      | Maintainer note                                                         |
| --------------------- | ---------- | ---------------------------------- | -------------------------------------------------------------- | ----------------------------------------------------------------------- |
| File attachments      | `✅ done`  | users can reference code and docs  | keep `@` path reference behavior stable                        | agents read referenced paths through ACP filesystem requests            |
| Local agent registry  | `✅ done`  | custom agents can be registered    | keep local config registration simple                          | ACP Registry should complement this, not replace it yet                 |
| Session lifecycle     | `✅ done`  | navigate previous sessions         | keep behavior stable while forking work continues              | `load`, `list`, `resume`, lazy indexing, and `undo` are already working |
| Session continuity    | `✅ done`  | users can continue prior work      | keep resume listing, speed, and metadata stable                | context/usage metadata and `session_info_update` are finished           |
| Session forking       | `🔵 now`   | sessions can branch and form trees | finish `fork` and tree-oriented flows                          | keep native ACP behavior ahead of local stand-ins                       |
| Image attachments     | `🔵 now`   | users can send visual context      | capability-aware image routing and transcript fidelity         | images reach ACP, but the path needs polish                             |
| Queued messages       | `🔵 now`   | users can keep typing during work  | finish backend handling for queued turns                       | TUI support exists, but ACP backend behavior needs tightening           |
| Session configuration | `🟡 next`  | agents can expose useful controls  | custom options like thinking/effort level and plan/build modes | keep this driven by agent-provided config, not hardcoding               |
| Agent discovery       | `🟡 next`  | custom agents are easier to find   | support the official ACP Registry                              | local config registration already works                                 |
| Agent-driven auth     | `⚪ later` | agents can own login/logout flows  | auth methods and `logout`                                      | wait until lifecycle, config, and registry are settled                  |
| Multi-session UX      | `⚪ later` | users can move across active work  | multi-session support and navigation                           | keep this separate from single-session lifecycle cleanup                |
| Steering messages     | `⚪ later` | users can redirect an active turn  | wait for ACP support, then design Nori behavior                | unsupported by ACP today                                                |
| Subagent UX           | `⚪ later` | group subagent updates             | wait for ACP support, then design Nori behavior                | unsupported by ACP today                                                |
| Experimental ACP      | `⚪ later` | adopt new protocol ideas carefully | elicitation, NES, `session/delete`, provider endpoint work     | do not let draft features complicate the core client                    |

## ACP Feature Appendix

This table keeps the roadmap grounded in the current ACP spec and draft surface. It is dependency-facing detail, not the main structure of the project roadmap.

| Product area          | ACP feature                                   | ACP status                   | Nori status           |
| --------------------- | --------------------------------------------- | ---------------------------- | --------------------- |
| Session lifecycle     | `session/load`                                | stable baseline              | done                  |
| Session lifecycle     | `session/list`                                | stable                       | done                  |
| Session lifecycle     | `session/resume`                              | unstable landed              | done                  |
| Session lifecycle     | `session/fork`                                | unstable landed              | todo                  |
| Session lifecycle     | `session/close`                               | unstable landed              | handled               |
| Session lifecycle     | `session/delete`                              | draft only                   | not planned near term |
| Session lifecycle     | undo, rewind, checkpoints                     | not first-class ACP today    | local only            |
| Session metadata      | `session_info_update`, context/usage metadata | stable + unstable landed     | done                  |
| Session metadata      | `messageId` and `userMessageId`               | unstable landed              | partial               |
| Turn control          | queued messages                               | client behavior              | in progress           |
| Turn control          | steering messages                             | ACP draft                    | later                 |
| Workspace             | `additionalDirectories`                       | unstable landed              | partial               |
| File attachments      | `@` path references + `readTextFile`          | ACP filesystem capability    | done                  |
| File attachments      | `ResourceLink` / embedded `Resource` context  | baseline + `embeddedContext` | todo                  |
| Image attachments     | image content blocks                          | prompt capability            | partial               |
| Configuration         | session config options                        | stable                       | near term             |
| Configuration         | boolean config options                        | unstable landed              | near term             |
| Discovery             | ACP Registry                                  | stable                       | near term             |
| Auth                  | auth methods                                  | unstable landed              | planned               |
| Auth                  | `logout`                                      | unstable landed              | planned               |
| Experimental protocol | elicitation                                   | unstable landed              | later                 |
| Experimental protocol | NES                                           | unstable landed              | later                 |
| Experimental protocol | custom provider endpoints                     | draft only                   | not planned near term |
| Experimental protocol | diff-delete metadata                          | draft only                   | not planned near term |
| Experimental protocol | proxy-chains                                  | draft only                   | not planned near term |
| Experimental protocol | subagents                                     | not first-class ACP today    | not planned near term |

## Notes

- This reflects our current understanding as of April 2026.
- "Done" means done enough for initial usage within Nori, not necessarily that every implementation detail is perfect.
- We will keep preferring fewer special cases and more native ACP behavior as we iterate.
