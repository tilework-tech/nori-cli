# Independent E1 verification

PR: https://github.com/tilework-tech/nori-cli/pull/653
Head: e9ff6232c3269111f720128dcde16389827b1abb
Base: feat/multi-session-d2-host

Real local nori / nori host / nori attach binaries with repository mock ACP processes. Terminal state captured through isolated tmux wrappers and rendered using Ghostty Web 0.4.0. Fonts: DejaVu Sans Mono, Noto Sans Symbols, Noto Sans Symbols 2. Capture delimiter removed before render. No product modifications.

The primary report is the single findings comment on PR #653. Individual subreports predate final synthesis. All requested functional scenarios passed. One intermittent should-fix diagnostic: normal child completion sometimes prints ACP agent command channel closed in the parent. Three positive captures: lifecycle-agent/depth-verified/01-grandchild-result, lifecycle-agent/capacity-verified/02-cap-reused, and local/queued/01-result-queued-while-working (before SIGKILL).

Follow-up attempts: 10 fresh nested runs, 15 ordinary direct delegates and 5 delegates into a streaming parent did not add occurrences. The exact asynchronous failing action is inferred from code; the visible error and missing HarnessActionFailed session scoping are confirmed.

Only terminal captures, test outputs, and sanitized message/process evidence are included. Raw ACP wire logs and local host credentials are excluded.
