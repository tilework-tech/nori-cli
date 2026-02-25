# CLI WIP

## Core

- harden the main loop + event loop
- harden the message passing interface
- externalize settings
- concurrent sessions/agents?

## ACP

- [x] support claude, codex, and gemini
- [x] add debugging and tracing
- [ ] agent observability (stderr capturing and connection status)
- [ ] session IDs and persistence
- [ ] authentication (auth token, login, logout)
- [ ] permissions
- [ ] isolation (seatbelt and bubblewrap?)
- [ ] health checks and failure tolerance

## UI

- conversation/history
  - markdown
  - tool calls
  - diffs
- permissions
- status and context
- fuzzy-finder and attachments

