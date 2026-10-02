# Nori CLI Docs

One prescriptive document per cross-cutting topic. Crate-level details live in
each crate's `docs.md`.

| Doc | Topic |
| --- | --- |
| [glossary.md](glossary.md) | Canonical terms for actors, turns, sessions, and tools |
| [turn-lifecycle.md](turn-lifecycle.md) | Turn state, `session/update` routing, and cancellation |
| [remote-control.md](remote-control.md) | Remote ACP transport behind `--remote` and `/remote-control` |
| [nori-client-mcp.md](nori-client-mcp.md) | The backend-owned `nori-client` MCP server and goals |
| [headless.md](headless.md) | `nori exec` plaintext mode and the ACP stdio facade |
| [transcripts.md](transcripts.md) | Transcript file locations and schema |

Rules for these docs:

- Describe current behavior in prescriptive terms. No status lines, history,
  implementation plans, or investigation logs; git history keeps those.
- A PR that changes behavior covered here updates the doc in the same PR.
- Add a new doc only for a topic that crosses crate boundaries and is easy to
  get wrong. Everything else belongs in the owning crate's `docs.md`.
