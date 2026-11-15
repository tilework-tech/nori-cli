# E1 lifecycle verification at e9ff6232

PASS: grandchild result returned through both ancestors; third-level request clearly refused.
PASS: max_sessions=2 rejects extra concurrent child; completion frees slot; subsequent child starts and completes; both completed histories remain listed and first child history opens.
PASS: explicit parent /close cancels descendants; three ACP subprocesses were sleeping/running before close and absent afterward (close/process-proof.json).

SHOULD-FIX: successful child completion sometimes prints red "ACP agent command channel closed" into parent history.
Observed independently in depth-verified/01-grandchild-result and capacity-verified/02-cap-reused (also /03-completed-history-retained).
Repro: enable multi_session and acp_proxy, mock-model parent; /delegate mock-model mock:delegate mock-model DEPTH_TWO_LEAF. Alternatively cap=2; permission-blocked child, reject another at cap, approve blocked child, return root, /delegate mock-model CAP_REUSED_AFTER_COMPLETION. Fast child completion triggers race intermittently. Root remains ready and receives correct completion.
Source: chatwidget/event_handlers.rs:376 submits LoadCustomPrompts when replaying SessionStarted; helpers.rs:404 emits HarnessActionFailed; app_event_sender.rs does not session-scope that event; app/event_handling.rs:974 displays it in current widget. Attribution path is confirmed in code; exact async LoadCustomPrompts task inferred from event ordering.

All nine PNG captures inspected against text. Fonts installed; trailing ANSI delimiter removed before Ghostty rerender. All e1-life-* sessions cleaned up using tui-stop. Wire proof and scripts retained. No product code, builds, pushes, or comments made by this worker.

## Scenario and selected capture table

| Scenario | Result | Capture directory |
|---|---|---|
| Depth-two result | PASS | `depth-verified/01-grandchild-result` |
| Grandchild listed/history | PASS | `depth-verified/02-three-sessions-history` |
| Third-level clear refusal | PASS | `depth-verified/03-depth-refused` |
| Concurrent cap refusal | PASS | `capacity-verified/01-full-cap-refusal` |
| Completed child frees slot | PASS + should-fix diagnostic | `capacity-verified/02-cap-reused` |
| Completed records retained | PASS | `capacity-verified/03-completed-history-retained` |
| Completed history opens | PASS | `capacity-verified/04-completed-child-viewable` |
| Descendants active before close | PASS | `close/01-descendants-running` |
| Explicit root close stops descendants | PASS | `close/02-descendants-closed` |

## Reproduction frequency and limits

Two independent initial positive observations: nested delegation after root prompt, and ordinary same-kind completion after freeing a permission-blocked cap slot. Follow-up 10 fresh nested runs (6 immediate; 4 after warm root prompt) showed 0 further errors, and all 10 completed. Thus this is an intermittent observed UI error; no deterministic frequency estimate is justified. `repeat-frequency.json` preserves results. The exact asynchronous failing action remains inferred; wrong-session HarnessActionFailed routing is explicit in code.

Minimal attempted reproduction: enable multi_session, select mock-model, send a root prompt, then `/delegate mock-model mock:delegate mock-model DEPTH_TWO_LEAF`. Watch parent during normal successful child teardown. The red `ACP agent command channel closed` appeared in a captured successful run; both descendant histories were closed and root remained ready.

Selected captures each have matching screen.txt, screen.png, corrected screen.ansi, and metadata.json. All nine were visually inspected; none selected during transition. Initial scratch assertion failures concerned expecting a task string that the mock does not echo and are not product failures.

Publish only selected capture directories and this report, `repeat-frequency.json`, `close/process-proof.json`, and sanitized `wire-process-proof.json`. DO NOT publish raw home/acp-wire logs: session/new advertises private local MCP bearer headers. All e1-life-* tmux sessions were cleaned up; no product files edited, no build/push/post performed.
