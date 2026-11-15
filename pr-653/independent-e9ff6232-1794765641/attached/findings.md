# Attached TUI verification — e9ff6232

All assigned attached scenario groups passed. No attached product bug confirmed.

Real binaries: nori-rs/target/debug/nori and mock_acp_agent built at e9ff6232. Real detached host listened at ws://127.0.0.1:18766/sessions. Real attach PTY used the isolated e1-attached tmux session, 120x40. For the two deferred navigation checks, proxy.py forwarded raw WebSocket frames and deliberately held only session-list requests; all delegation, event, permission, and prompt traffic continued through the real host. The session used mock-model and mock-model-alt. No product code changes.

| Requirement | Outcome | Evidence directory |
|---|---|---|
| Same-agent delegation starts without asking | PASS | 01-same-agent |
| Cross-agent asks before starting; approve and decline | PASS | 02-cross-approval, 03-cross-declined, 04-child-permission-notice |
| Child permission names child and parent; no parent popup | PASS | 04-child-permission-notice |
| Alt+Down opens actual child and answer routes to child | PASS | 05-child-permission, 06-child-permission-answer |
| Automatic result returns to parent | PASS | 07-parent-result |
| Fresh child before attached list periodic refresh | PASS | 08-fresh-child |
| Deferred Alt+Down while typing keeps draft in parent | PASS | 09-deferred-parent-draft |
| Current row keeps short ID in fixed ID column | PASS | 10-current-id-list |
| Host ls child rows keep fixed ID/AGENT/STATUS/AGE; parent in TITLE; terminal truncates long title | PASS | 11-host-ls-columns, host-ls-piped.txt |
| Host restart rotates token and rereads local token; clear stopped explanation; draft retained | PASS | 12-host-restart |
| Fresh attach reaches replacement host, sessions empty | PASS | 13-fresh-attach-after-restart |

Ownership proof from proxy.jsonl:
- Parent: 5c56204d-2593-4555-be2b-72662d064bc5.
- Cross-agent permission child: 73f4af00-8d11-44ba-aec8-23c144aeb929. `_nori/session/respond` request id64 names this child, with optionId allow.
- Fresh child: e2b685b0-3a42-40ec-ba7c-beb5b3166594. Held list request until after child's permission was known to the host, pressed Alt+Down while cached rows lacked that child, then released list request. One key selected child and showed its permission.
- Deferred child: 503e47c4-5abd-4c7a-afa6-26a378014c21. Held list request; delegated child, pressed Alt+Down; typed PARENT-DRAFT-MUST-STAY-HERE; released list request. Parent stayed selected. `_nori/session/prompt` id218 delivered exact draft to parent ID.
- Fixed-column assertions used every piped row: ID bytes0..36, AGENT38..54, STATUS56..72, AGE74..80, TITLE82 onward. Piped output kept full task; 120-column terminal used ellipsis and kept parent prefix.
- Restart used `host stop`, then `host --port 18766`; SHA256 comparison confirmed token changed (token content not printed). Existing local attach displayed `Host restarted; sessions were lost. Re-run nori attach.` and preserved draft. New attach displayed Sessions 0.

All 14 PNGs (00 through13) were visually inspected against their text. Ghostty capture used installed DejaVu Sans Mono/Noto symbols fonts. screen.raw.ansi retains raw wrapper output; exactly the final LF was removed from screen.ansi before rerender so top header remains visible.

Fixture-only retries: initial nohup proxy died with command process group; setsid fixed lifetime. A long command sent before its complete composer render left the last H in draft, so Alt+Down appropriately refused to switch; driver was corrected to assert text before Enter. Another fixture asserted a task marker in parent history though only notices display IDs, delaying held lists beyond transport timeout; removed that assertion and repeated bounded deterministic checks. Neither is a product finding.

Owned host, proxy PID71168, e1-attached and e1-attached-ls sessions stopped after verification. Single-session feature-off is main verifier's scope.
