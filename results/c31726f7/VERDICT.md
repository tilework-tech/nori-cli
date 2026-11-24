# #751 `c31726f7` — shared slash-command table (parent #750 `32471d77`)

All data is synthetic. Every scenario uses real tmux PTYs.

## New scenarios (S35, S36): 56/59 at #751, 34/59 at #750

| Scenario | #751 | #750 | Notes |
|---|---|---|---|
| S35a plain `/` menu, Tab, dispatch, agent commands | 9/10 | 9/10 | Plain `nori` namespaces agent commands as `/claude-code:review`, and those reach the agent once. A bare `/review` opens `/resume-viewonly` on both heads (pre-existing). |
| S35b attach controller | 22/22 | 8/22 | Host-route and attach-screen reasons. Refusals keep the draft and send nothing. Tab completes builtins. `/review`, `/review the parser` and `/lanternize` are each sent once. ` quit` is sent as a prompt; bare `quit` detaches and the session stays. Each refusal still shows twice (known). |
| S35c watcher and control transfer | 10/10 | 5/10 | Watcher sees `watching · Alt+T to take control` on `/new`, `/close`, `/resume` and `/delegate`. Its `/new` is refused, kept, and not sent. An open controller menu re-greys when the watcher takes control. The new controller's `/new` runs once. |
| S35d/e Escape mid-turn with menu open | 3/4, 4/5 | 3/4, 2/5 | **Finding:** Escape never closes the `/` menu (plain on both heads; attach from #751). The turn is not cancelled either, so in attach Escape does nothing until the `/` is deleted. Before #751, Escape in attach cancelled at once. |
| S35f/g paste | 2/2, 2/2 | 2/2, 1/2 | A pasted `/compact` is refused like a typed one. A multi-line paste starting with `/diff` is sent once as a prompt in plain `nori` and in attach. |
| S35h reconnect | 6/6 | 3/6 | The open menu reads `reconnecting`. `/new` is kept and not sent while the link is down, then runs once after reconnecting. The notice reads "Reconnecting. Your draft is preserved…", not `/new: reconnecting`, and is shown twice. |
| S36 startup Enter sweep | 2/2 | 2/2 | 18 trials per head, all delivered once (Linux). |

## Full matrix

400/430 at #751 against 394/430 at #750, checked one by one. Nothing
regressed. Six checks now pass: S05 slash discovery (5) and
`S10 approvals.not_sent_as_prompt`.

## Native macOS

Shares #750's startup failure; see `../32471d77-race/NOTE.md`. The traced
native repeat passed 8/8 (`host_startup` + `host_new_session`, run
45394569051). The welcome-card test failure in that run comes from my keeping
homes inside the checkout, not from the product.

## Verdict

Changes requested, for one finding plus the inherited #750 startup race:
1. Escape on an open `/` menu should close it. In attach the menu is new, and
   it now swallows the Escape that used to cancel a turn. Root cause: the
   non-vim Escape branch in `chat_composer/key_handling.rs` clears
   `active_popup` but does not record `dismissed_command_popup_text`, so the
   next sync reopens the menu for the same `/` text (likely cause; the vim Normal branch records it and stays closed).
2. Restack on the #750 fix and repeat the native startup suites.
