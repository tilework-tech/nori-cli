# Sessions, History, and Continuity

## Identity

| Term | Definition | Aliases to avoid |
| --- | --- | --- |
| **Session** | An ACP conversation context with its own history and state. | Process, connection |
| **ACP session ID** | The opaque agent-issued identifier used in ACP requests for one session. | Conversation ID |
| **Conversation ID** | Nori's local UUID for a transcript-backed conversation, independent of its ACP session ID. | Session ID, ACP session ID |
| **Continuity** | Nori's preservation of prior work across session and context changes. | Persistence, replay |

## Session lifecycle

| Term | Definition | Aliases to avoid |
| --- | --- | --- |
| **New** | ACP `session/new` creates an independent session and returns its ACP session ID. | Resume, reconnect |
| **List** | ACP `session/list` discovers agent-known sessions without restoring or modifying them. | History, load |
| **Load** | ACP v1 `session/load` restores a session and replays its conversation history before responding. | Resume, transcript replay |
| **Resume** | ACP v1 `session/resume` restores a session without replaying prior messages. | Load, Nori resume |
| **Nori resume** | Nori's user action chooses **Load**, **Resume**, or **New** plus transcript fallback from identity and agent capabilities. | `session/resume`, reload |
| **Close** | ACP `session/close` cancels in-flight work and releases active agent resources without deleting the session. | Quit, detach, delete |

## History and continuity operations

| Term | Definition | Aliases to avoid |
| --- | --- | --- |
| **Conversation history** | The agent-maintained prior content and context of a session. | Transcript, session list, prompt history |
| **Transcript** | Nori's versioned local JSONL record of metadata, user input, and ordered events. | Conversation history, ACP log |
| **Replay** | Filtered historical ACP notifications that never re-execute requests or side effects. | Resume, rerun, transcript |
| **Fork** | Optional, unstable ACP `session/fork` creates a child from existing session context. | Rewind, Git branch |
| **Branch at head** | Nori forks the active ACP session and transcript, activates the child, and freezes the resumable parent. | Rewind, undo |
| **Rewind to message** | Nori's older-message `/fork` path starts fresh from a display summary and prefills that message. | ACP fork, undo |
| **Compaction** | Nori reduces model context through agent-native compaction or summary-and-session-swap. | History deletion, replay |
| **Undo** | Nori restores a Git ghost snapshot without changing agent context or transcript. | Rewind, rollback conversation |

## Relationships

- One **Conversation ID** may outlive multiple **ACP session IDs**, notably after fallback **compaction**.
- **Load** produces agent-sourced **replay**; Nori's fallback uses transcript-sourced replay after **New**.
- A **Transcript** records selected session traffic but is not the agent-maintained **conversation history**.
- **Close** releases an active session; quitting may only detach, and neither operation means delete.
- **Undo** changes files, not conversation state; **Rewind to message** changes conversation direction, not files.

## Example dialogue

> **Dev:** "Does `nori resume` always send ACP **Resume**?"
>
> **Domain expert:** "No. **Nori resume** may use **Load**, live **Resume**, or **New** plus transcript-sourced **replay**."
>
> **Dev:** "After **Undo**, does the agent forget the reverted turn?"
>
> **Domain expert:** "No. **Undo** restores files only; use **Rewind to message** or **Branch at head** to change direction."

## Flagged ambiguities

- "Session ID" is unsafe alone; say **ACP session ID** or **Conversation ID**.
- "Resume" must distinguish the **Nori resume** action from ACP **Resume** and **Load**.
- "History" may mean **conversation history**, a **Transcript**, the session list, or composer prompt history; qualify it.
- `/fork` exposes both **Branch at head** and **Rewind to message**, but only the former uses ACP **Fork**.
- **Compaction** may replace the ACP session without creating a new Nori conversation, while **Undo** never rewinds agent context.
